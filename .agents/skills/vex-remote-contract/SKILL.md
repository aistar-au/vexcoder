---
name: vex-remote-contract
description: >
  Batch dispatch and branch-verification workflow for GitHub repos. Use this skill whenever the
  user wants to read raw/blob GitHub URLs, produce dispatch markdown with dependency gates and
  anchor tests, verify branch content through raw URLs or a .diff URL, generate a verification URL
  map, maintain the repo-wide raw URL index for newly added files, draft a PR motivation body, or
  run the end-to-end loop:
  read → dispatch → verify → push → raw-url-check → diff-check → merge.
---

# Vex Remote Contract Skill

An end-to-end skill for the **read → dispatch → verify → push → raw-url-check → diff-check → merge** loop used in Rust repo automation. Works with any locally-running coding agent.

**Confirmation required before any remote change.** When a prompt requests changes to
a remote branch, file, or PR, stop and present the full planned change to the user
before executing. Do not push, commit, or call any write API until the user has
explicitly confirmed. A statement of intent ("I will do X") is not confirmation —
explicit user approval ("yes", "go ahead", "confirmed") is required. If confirmation
is not received, do not proceed.

**All changes must be exact diffs applied as patches.** Never edit a skill file or any
other repository file by rewriting it in full from memory or a cached copy. The
required workflow is: (1) fetch the current file content from the remote branch,
(2) produce a precise unified diff showing only the lines that change, (3) present
the diff to the user for review, (4) apply the patch by writing the new content that
results from applying the diff. Any change that cannot be expressed as a diff against
the current remote file content must not be applied.

## Patch Hunk Standard (required)

For any hunk-level repository change in this workflow, use exact unified diffs
applied with `git apply` only.

Required format:

```diff
diff --git a/<path> b/<path>
--- a/<path>
+++ b/<path>
@@ -<old_start>,<old_count> +<new_start>,<new_count> @@
-<old line>
+<new line>
```

Required application sequence:

```sh
cat > /tmp/<n>.patch <<'PATCH'
<exact unified diff>
PATCH

git apply --check --recount /tmp/<n>.patch
git apply --recount /tmp/<n>.patch
```

Do not reconstruct or overwrite whole files to apply a hunk. Do not use non-diff
editing methods for hunk-level changes.

---

## Overview of the Loop

```
Step 0   SYNC      Update local branch from remote before any verification/read
Step 0.5 RECOVER   Resurrect deleted branch from dangling commits (when applicable)
Step 1   READ      Fetch raw GitHub URL(s) from a branch or main
Step 2   DISPATCH  Write the batch dispatch prompt (markdown only, no plain text)
Step 3   VERIFY    Second-agent review of dispatch; apply corrections
Step 4   EXECUTE   Agent writes code, runs cargo test, pushes branch
Step 5   URL MAP   Generate /tmp/<branch>-verification-urls.md
Step 6   RAW CHECK Fetch every raw URL → HTTP 200 + content match
Step 7   DIFF CHECK Fetch .diff URL → confirm all expected paths present
Step 8   CI GREEN  clippy/rustfmt/tests + GitHub Actions pass
Step 9   MAP GATE  Update/check TASKS/completed/REPO-RAW-URL-MAP.md for new files
Step 10  MERGE     Merge commit (no squash, no rebase) → verify via raw map URL
```

Always output **pure markdown** when producing dispatch prompts or reports. Never emit plain prose paragraphs in dispatch output.

---

## Step 0 — Sync Local Before Verification

Before reading files, checking anchors, validating hunks, or producing gate
status, establish the correct branch context. If the working tree is dirty,
stop and report it — do not attempt checkout over uncommitted changes.

```sh
# Fetch and prune in one step. Do not run git pull before this.
git fetch origin --prune

# Checkout the actual verification target.
# Only use main here when main is explicitly the subject.
git checkout <target-branch>
# Use the already-fetched ref — avoids a second network call and does not
# depend on local branch tracking config being set.
git merge --ff-only origin/<target-branch>
```

Always report the head SHA used for verification.

---

## Step 0.5 — Branch Recovery (Deleted Branch)

If a branch was deleted but its tip SHA is still reachable (dangling commit retained
by GitHub for ~90 days, or known from a prior session), recover it before proceeding.

### Recovery priority ladder

Use the highest-priority method available. Do not attempt a lower-priority method
until the higher-priority path has been confirmed unavailable or has failed.

| Priority | Method | Condition |
| :--- | :--- | :--- |
| 1 — lowest latency | Local `git`/`gh` CLI — cherry-pick + push | **Always try first.** Requires local git access. |
| 2 | MCP `push_files` — single batched call | Local CLI unavailable **and** total payload < 50 KB. |
| 3 — highest latency | `git apply` unified diff patch | Last resort. Never use when cherry-pick is viable. |

For payloads of ~2k lines across 15+ files, Priority 1 (local CLI) is the only
practical option. Attempting MCP or `git apply` at that scale wastes significant
round-trip time. Do not proceed to Priority 2 or 3 until the local path is
confirmed unavailable or has explicitly failed.

---

### Priority 1 — Local CLI recovery (preferred)

Present the following script to the user as a **paste-and-run terminal prompt**.
Fill in `TARGET` and `OLD_TIP` from the session context, then ask the user to
run it locally before any MCP action is attempted.

```zsh
#!/usr/bin/env zsh
# Branch recovery from dangling commit.
# Run from your repo root. Fill in TARGET and OLD_TIP before running.

TARGET="dispatcher/adr-023-el-01-el-02-runtime-only"
OLD_TIP="<full-sha>"   # dangling commit SHA from prior session or review

set -euo pipefail

# 1. Fetch the dangling commit object from the remote (works while GitHub retains it).
git fetch origin "$OLD_TIP" 2>/dev/null || true
git fetch origin --prune

# 2. Create a local recovery ref pointing at the dangling commit.
git branch -f recover/temp "$OLD_TIP"

# 3. Switch to the target branch (creates a local tracking branch if absent).
git checkout "$TARGET" 2>/dev/null \
  || git checkout -b "$TARGET" "origin/$TARGET"

# 4. Inspect what the recovery ref adds over the current target HEAD.
echo "=== Commits to recover ==="
git log --oneline "$TARGET..recover/temp"
git cherry -v "$TARGET" recover/temp

# 5. Cherry-pick the dangling commit(s) onto the target branch.
#    Replace <sha> with the commit SHA(s) printed above.
git cherry-pick <sha>
#
# To collapse multiple commits into one squashed commit:
#   git cherry-pick --no-commit <sha1> <sha2> <sha3> && \
#   git commit -m "Recover <description> from deleted branch"

# 6. Push the restored branch.
git push origin "$TARGET"

# 7. Verify push landed (Hard Rule 10).
git fetch origin --prune
LOCAL_SHA="$(git rev-parse HEAD)"
REMOTE_SHA="$(git rev-parse "origin/$TARGET")"
echo "Local : $LOCAL_SHA"
echo "Remote: $REMOTE_SHA"
test "$LOCAL_SHA" = "$REMOTE_SHA" \
  && echo "VERIFIED - SHAs match" \
  || { echo "MISMATCH - do not proceed"; exit 1; }

# 8. Clean up recovery ref.
git branch -D recover/temp
```

**Verification checkpoint — required before any MCP fallback**

After presenting the script above, pause and ask the user to confirm:

> _"Please run the recovery script above in your local terminal and paste the step 7
> output (both SHAs and the VERIFIED or MISMATCH line). I will not attempt MCP
> push_files until you confirm the local path has been tried."_

Do not proceed to Priority 2 or 3 until the user either:

- (a) pastes the step 7 verification output showing a mismatch or a run error, or
- (b) explicitly states that local git access is unavailable in their environment.

This checkpoint is mandatory when the payload is ~2k lines or 15+ files.

---

### Priority 2 — MCP `push_files` (< 50 KB total payload only)

Use only after the Priority 1 verification checkpoint is satisfied and total file
payload is confirmed below 50 KB.

A single large file (e.g. `Cargo.lock` at ~65 KB) can exceed the threshold alone.
Sum the `size` field on every blob returned before committing to this path.

1. Recreate the branch from main with `github:create_branch` if absent on remote.
2. Sum blob `size` fields for all changed files. If the total is >= 50 KB, stop —
   escalate to local CLI access (Priority 1). Do not use this path.
3. Push **all files in a single `github:push_files` call** — never loop one file per call.
4. Confirm the restored HEAD SHA by re-fetching at least one key blob and comparing.

---

### Priority 3 — `git apply` unified diff patch (last resort)

Use only when Priority 1 and Priority 2 are both confirmed unavailable. This method
has the highest per-file latency because each changed file must be expressed as a
unified diff hunk and applied via `git apply` separately.

For payloads exceeding ~10 files or ~500 lines the cumulative round-trip cost is
prohibitive. Escalate to local CLI access instead of proceeding here.

Follow the **Patch Hunk Standard** at the top of this skill exactly:

```sh
cat > /tmp/<n>.patch <<'PATCH'
<exact unified diff>
PATCH

git apply --check --recount /tmp/<n>.patch
git apply --recount /tmp/<n>.patch
```

Never use `git apply` to recover large dangling commits (~2k lines, 15+ files).

---

### Verification report (required after recovery push via any method)

```markdown
**Repo:** `<owner/repo>`
**Target branch:** `<branch-name>`
**Recovered commit SHA(s):** `<sha>`
**Recovery method used:** Priority <1 | 2 | 3>
**Local HEAD SHA:** `<sha>`
**origin/<branch> SHA:** `<sha>`
**Verification:** match
```

If local HEAD and origin SHA do not match, stop and report mismatch — do not
proceed to PR creation.

---

## Step 1 — READ Remote Content

When given a raw GitHub URL (e.g. `https://raw.githubusercontent.com/...` or a blob URL ending in a file path):

1. Use the available HTTP fetch method to retrieve the content. Scripts use a 3-tier cascade: `curl` (primary, HTTP code checked) → `wget` (fallback on images without curl) → `gh api` with `Accept: application/vnd.github.raw` (fallback when CDN is firewalled).
2. Identify the repo slug (`owner/repo`), branch, and file path.
3. If the URL is a blob URL (`/blob/`), rewrite to raw (`/raw/` or `https://raw.githubusercontent.com/<slug>/<branch>/<path>`).
4. Parse any TASKS file, ADR, or map document to extract the task manifest for the upcoming batch.

```
Pattern — blob → raw rewrite:
  https://github.com/owner/repo/blob/<branch>/path/to/file.md
  → https://raw.githubusercontent.com/owner/repo/<branch>/path/to/file.md
```

---

## Step 2 — DISPATCH Prompt Format

Every dispatch prompt must be a self-contained markdown document. No plain text. Required sections:

### Dispatch Template

````markdown
## Batch <X> Dispatch — <ADR-NNN> <Phase range> [(corrected)]

**Baseline:** `origin/main` at `<short-sha>`

**Working branch setup — explicit remote ref to avoid stale local:**
```sh
git fetch origin --prune
git checkout -b <branch-name> origin/main
```
> `git checkout main && git pull` is not sufficient if local `main` is behind.
> Branch directly from `origin/main`.

---

### Dependency graph

```
<ROOT>
  └── <TASK-A>
        └── <TASK-B>
              ├── <TASK-C>
              └── <TASK-D>  ← [note any non-obvious dependency here]
```

### Execution order

| Step | Task | Notes |
| :--- | :--- | :--- |
| 1 | TASK-A | entry point |
| 2 | TASK-B | after TASK-A anchor green |
| … | … | … |

Stop at the first anchor that is **not green**. Do not proceed to its dependents.

---

### <TASK-ID> — <Title>

**Target files:** `path/to/file.rs` (new or modified), …

**Gate:** <PREV-TASK> anchor green [if already done]

**What to implement:**

[implementation spec — types, traits, structs, env var names, parse rules, constraints]

**Anchor test** (location `src/…` `#[cfg(test)]` or `tests/…`):
```rust
#[test]  // or #[tokio::test]
fn test_<descriptive_name>() {
    // …
}
```

**Verification:**
```sh
cargo test --all-targets
cargo test <anchor_test_fn_name> --all-targets
```

---

### Hard constraints

- Branch from `origin/main` using `git fetch origin --prune && git checkout -b <branch> origin/main`.
- Merge to `main` using a **merge commit only** — no squash, no rebase.
- Stop immediately if any anchor is **not green**. Do not proceed to dependents.
- [list any project-specific invariants here]

---

### Required final report

1. **Task results** — `green` or `not green` for each task ID
2. **Files changed** — full list of paths
3. **Verification run** — each anchor test command with exit code
4. **Open issues** — exact compile error or test failure for anything not green

If any task is not green, all tasks that depend on it must say `not attempted`.
````

---

## Step 3 — Dispatch Verification

Before executing, pass the dispatch to a second agent or review pass. Frame the request as:

```
go ahead follow this exactly and do not make any other changes:
<paste corrected dispatch>
```

The reviewing agent must check:
- Dependency graph is internally consistent (no cycles, no missing gates)
- Execution order matches the graph
- All `Gate:` references name a real preceding task
- No task spec contradicts a hard constraint
- Anchor tests are syntactically valid Rust

Return a corrected dispatch (labelled `(corrected)`) if any issues are found.

---

## Step 4 — EXECUTE

The coding agent follows the dispatch:

1. `git fetch origin --prune && git checkout -b <branch> origin/main`
2. Implement each task in execution order, stopping on any red anchor.
3. Run `cargo test --all-targets` after each task.
4. Before push, run the local CI gate:

```sh
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo test --all-targets
```

If any command fails, fix it before commit/push.
5. Commit with message: `Batch <X>: <ADR-NNN> Phases <range> implementation`
6. `git push origin <branch>`
7. Ensure push landed by verifying local and remote SHAs match:

```sh
git fetch origin --prune
LOCAL_SHA="$(git rev-parse HEAD)"
REMOTE_SHA="$(git rev-parse origin/<branch>)"
test "$LOCAL_SHA" = "$REMOTE_SHA"
```

8. Generate `/tmp/<safe-branch-name>-verification-urls.md` (see Step 5 script).

---

## Step 5 — Generate Verification URL Map

Run from repo root. Produces `/tmp/<branch>-verification-urls.md`.

```bash
bash .agents/skills/vex-remote-contract/scripts/gen_verification_urls.sh \
  -b <branch> \
  --base origin/main
```

The output file lists every changed file as a raw GitHub URL grouped by category (Cargo & Config / Source / Tests / Other). It also records commit SHA, ahead/behind counts, and a clean working-tree assertion.

**Manual equivalent** (if script unavailable):

```bash
git fetch origin --prune
branch="<branch>"
repo_slug="$(git remote get-url origin | sed -E 's|.*github\.com[:/]||;s|\.git$||')"
git diff --name-only "origin/main...origin/$branch" | while read -r f; do
  echo "https://raw.githubusercontent.com/$repo_slug/$branch/$f"
done
```

---

## Step 6 — RAW URL Verification

Paste the raw URLs into the next agent (one URL per line appended to the prompt). The agent must:

1. Attempt fetch via 3-tier cascade per file:
   - **curl** (primary): `curl -L -sS -w '%{http_code}'` — HTTP code must be `200`.
   - **wget** (fallback): used when curl is absent (minimal Linux images).
   - **gh api** (fallback): `gh api -H "Accept: application/vnd.github.raw"` with `GH_HTTP_TIMEOUT` set — used when CDN is firewalled but API is reachable.
2. A file fails only if all three methods fail or return non-200.
3. If `--compare` mode: compare SHA-256 of fetched content against `git show origin/<branch>:<file>`.
4. Report `[x] OK <method>` or `[ ] FAIL` for every file.
5. Emit `**PASS**` only when all files pass.

```bash
bash .agents/skills/vex-remote-contract/scripts/verify_raw_urls.sh \
  -b <branch> \
  --compare
```

**Only raw GitHub URLs are appended to agent prompts at this stage.** Do not paste full file content.

---

## Step 7 — DIFF URL Verification

Fetch the `.diff` URL for the PR or compare view:

```
https://github.com/<owner>/<repo>/compare/main...<branch>.diff
# or for a PR:
https://patch-diff.githubusercontent.com/raw/<owner>/<repo>/pull/<N>.diff
```

```bash
bash .agents/skills/vex-remote-contract/scripts/verify_diff_url.sh \
  -b <branch> \
  -u "https://github.com/<owner>/<repo>/compare/main...<branch>.diff"
```

The script parses all `diff --git a/<path>` headers and confirms every file from the verification URL map appears in the diff. Missing files → fail.

---

## Step 8 — CI Green Gate

Before merging:

- Local CI gate passes:
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo fmt --check`
  - `cargo test --all-targets`
- GitHub Actions workflow is green (no red jobs).
- Resolve any conflicts with the following, then re-run anchor tests:
  `git fetch origin --prune && git merge origin/main`
- Working tree must be clean (`git status --porcelain` → empty).

Do not create a PR until the branch is conflict-free and CI is green.

### CI failures playbook (common CI failures)

When CI fails on clippy in this repo, apply these exact transformations before re-running checks:

1. `clippy::unnecessary_lazy_evaluations`
   - `opt.unwrap_or_else(|| <constant/cheap value>)` → `opt.unwrap_or(<value>)`
2. `async_fn_in_trait`
   - Public trait methods: `async fn` → `fn ... -> impl Future<Output = ...> + Send`
   - Keep `async fn` in trait impl blocks.
3. `clippy::should_implement_trait`
   - Do not define ambiguous inherent methods like `fn default()`.
   - Implement `Default` trait directly and move shared init into a helper with a non-conflicting name.
4. `clippy::map_entry`
   - Replace `contains_key` + `insert` with `entry(...).or_insert(...)`
5. `clippy::manual_clamp`
   - Replace `.max(a).min(b)` with `.clamp(a, b)`

---

## Step 9 — Repo Raw URL Map Gate

Project policy:

- `TASKS/TASKS-DISPATCH-MAP.md` is the descriptive dispatch contract document for this repo.
- `TASKS/completed/REPO-RAW-URL-MAP.md` is the canonical whole-repo raw URL file map.
- Keep this map synchronized with repository files.
- Update it on any drift (new/missing files, line-count drift, URL drift, header totals).

Check map coverage (required before push/PR):

```sh
bash .agents/skills/vex-remote-contract/scripts/update_repo_raw_url_map.sh --check
```

If the check reports drift, regenerate it:

```sh
bash .agents/skills/vex-remote-contract/scripts/update_repo_raw_url_map.sh
```

Then verify again:

```sh
bash .agents/skills/vex-remote-contract/scripts/update_repo_raw_url_map.sh --check
```

If no drift is present, update script prints a no-op message and leaves the file untouched.

---

## Step 10 — PR Body File and Merge

Generate the PR summary and write a markdown body file in `/tmp`:

```sh
bash .agents/skills/vex-remote-contract/scripts/branch_summary.sh \
  -b <branch> \
  --write-pr-body
# writes: /tmp/<safe-branch-name>-pr-body.md
```

Optional custom output path:

```sh
bash .agents/skills/vex-remote-contract/scripts/branch_summary.sh \
  -b <branch> \
  --write-pr-body \
  -o /tmp/<custom>-pr-body.md
```

Create the PR using the generated markdown file:

```sh
gh pr create \
  --base main \
  --head <branch> \
  --title "Batch <X>: ADR-<NNN> <short title>" \
  --body-file /tmp/<safe-branch-name>-pr-body.md
```

When CI is green and review is complete, merge with a **merge commit** only:

```sh
git checkout main
# Sync local main with remote before merging — push will be rejected if behind.
git fetch origin --prune && git merge --ff-only origin/main
git merge --no-ff <branch> -m "Merge batch-<x>/adr-<nnn>: <short description>"
git push origin main

# Verify main landed — required by Hard Rule 10.
git fetch origin --prune
LOCAL_SHA="$(git rev-parse HEAD)"
REMOTE_SHA="$(git rev-parse origin/main)"
test "$LOCAL_SHA" = "$REMOTE_SHA"

# Commit hygiene gate — batch promotions must land as merge commits on main.
PARENT_COUNT="$(git rev-list --parents -n 1 HEAD | awk '{print NF-1}')"
test "$PARENT_COUNT" -ge 2
```

---

## Step 11 — Post-Merge Verification

After merge, open the report with `Repo: <owner/repo>` and `Verified at commit: <full-sha>`.
Re-fetch the original raw map URL (the TASKS or ADR document on main) and confirm:

- The dispatch tasks are addressed by files present at commit `<full-sha>` on `main`.
- `git log --oneline -5` shows the merge commit.
- `cargo test --all-targets` is green on `main`.
- Every claimed file is verified at commit `<full-sha>` via `git ls-tree HEAD <path>`
  and reported with blob SHA; do not infer file state from commit membership alone.

### Direct push exception flow (required when applicable)

For batch promotions, direct pushes to `main` are hygiene failures by default.

If a direct push is explicitly requested, stop and capture explicit user confirmation
before pushing, then include this exception record in the final report:

```markdown
### Commit Hygiene Exception
- Repo: `<owner/repo>`
- Type: direct push to `main`
- Scope: `<start-sha>..<end-sha>`
- Reason: `<why merge-commit flow was bypassed>`
- Approval: `<explicit user confirmation text or link>`
- Follow-up: `<how normal merge flow will resume>`
```

---

## Scripts Reference

All scripts live in `.agents/skills/vex-remote-contract/scripts/`. Bootstrap them with:

```bash
mkdir -p .agents/skills/vex-remote-contract/scripts
# copy scripts (see bundled files)
git add .agents/skills
git commit -m "Add branch contract skill scripts"
```

| Script | Purpose |
| :--- | :--- |
| `_lib.sh` | Shared helpers (`die`, `repo_slug_from_origin`, `sha256_file`) |
| `gen_verification_urls.sh` | Generate raw URL map → `/tmp/<branch>-verification-urls.md` |
| `verify_raw_urls.sh` | HTTP-check every raw URL; optionally compare content vs git ref |
| `verify_diff_url.sh` | Confirm .diff URL contains all expected file paths |
| `update_repo_raw_url_map.sh` | Check/update `TASKS/completed/REPO-RAW-URL-MAP.md` for new files |
| `branch_summary.sh` | Print summary and optionally write `/tmp/<branch>-pr-body.md` |

### Key flags

| Flag | Meaning |
| :--- | :--- |
| `-b / --branch <n>` | Branch to operate on (inferred from HEAD if omitted) |
| `--check` | `update_repo_raw_url_map`: fail if repo map misses tracked files |
| `--force` | `update_repo_raw_url_map`: regenerate map even without missing files |
| `--map <path>` | `update_repo_raw_url_map`: alternate raw map path |
| `--repo-slug <owner/repo>` | `update_repo_raw_url_map`: override repo slug |
| `--write-pr-body` | `branch_summary`: write markdown PR body to `/tmp` |
| `-o / --out <path>` | `branch_summary`: custom output path for PR body markdown |
| `--base <ref>` | Comparison base (default: `origin/main`) |
| `--compare` | `verify_raw_urls`: also SHA-compare content vs git ref |
| `-u / --url <url>` | `verify_diff_url`: the `.diff` URL to fetch |
| `--urls-only` | `gen_verification_urls`: emit one raw URL per line, no decoration |
| `--timeout <sec>` | curl timeout (default 20–30 s) |

---

## Hard Rules (Universal — Apply to Every Batch)

1. **Always branch from `origin/main`** via `git fetch origin --prune && git checkout -b <branch> origin/main`. Never from local `main`.
2. **Merge commit only** — `git merge --no-ff`. No squash. No rebase.
3. **Stop on red anchor** — never proceed to a dependent task if its gate is not green.
4. **`ENV_LOCK.blocking_lock()`** in all sync tests — `.lock().unwrap()` will not compile against `tokio::sync::Mutex`.
5. **Working tree must be clean** before any verification script runs.
6. **Only raw GitHub URLs** in agent prompts during Step 6. No full file content paste.
7. **All output is markdown** — no plain text paragraphs in dispatch or report documents.
8. **Repo map gate required** — run `update_repo_raw_url_map.sh --check`; if drift is reported, update then re-check.
9. **Final report required** — every batch must close with task results table, files changed, verification commands with exit codes, and open issues.
10. **Ensure push landed** — after every `git push`, run `git fetch origin --prune` and confirm `git rev-parse HEAD` equals `git rev-parse origin/<branch>`.
11. **Commit hygiene gate required** — batch promotions on `main` must end on a merge commit (`git rev-list --parents -n 1 HEAD` parent count `>= 2`).
12. **Direct push requires explicit exception record** — if `main` is updated without merge commit flow, include a `Commit Hygiene Exception` section with scope, reason, approval, and follow-up.
13. **File-level commit verification required** — for each claimed file, report the blob SHA and the exact commit SHA from `HEAD` (`git ls-tree HEAD <path>`). Reference the commit SHA directly; do not use informal status labels in place of identifiers.
14. **Full repo slug required in every transaction report** — every verification report, push confirmation, merge record, and exception record must identify the repository as `owner/repo` (e.g. `aistar-au/vexcoder`) in the opening line. Bare repo names and local path references are not permitted.
15. **Code review gate required (Step 6.5)** — no merge and no next-batch dispatch until all CHANGES_REQUESTED items from Step 6.5 are resolved. Blob SHA verification and anchor test presence are not a substitute for reading implementation content.
16. **Load skills before any action** — at the start of every session, load and read both `.agents/skills/vex-remote-contract/SKILL.md` and `.agents/skills/github-pr-review/SKILL.md` in full before writing any dispatch, diff, or review output. Do not proceed past the Bootstrap section until both files have been read completely.
17. **Emojis forbidden in all output** — no emoji, Unicode symbol, or icon in any position in any text produced by this skill. Use GitHub API state labels only: `CHANGES_REQUESTED`, `COMMENT`, `APPROVED`, `resolved`, `open`. This rule applies to every output channel: review bodies, dispatch docs, findings tables, inline comments, commit messages, PR titles, PR bodies, log lines, and script output.
18. **Confirmation required before remote writes** — before any push, commit, file write, or write API call, present the full planned change to the user and wait for explicit confirmation. A description of intent is not confirmation. Do not proceed until the user responds with explicit approval.
19. **Exact diffs only — no full-file rewrites** — all file changes must be created as a precise unified diff against the current remote content and applied as a patch. The required steps are: fetch current content, produce diff, present diff for review, apply patch. Reconstructing a file from memory or a cached copy is not permitted under any circumstance.
20. **Hunk patches must use `git apply` only** — prepare exact unified diffs and run `git apply --check --recount` before `git apply --recount`.
21. **No file reconstruction for hunk edits** — do not rewrite complete files from memory or cached content when a focused patch hunk is required.
22. **Push method: count KB not lines** — when committing recovered or batch files, use `gh` CLI locally (cherry-pick + push) when the total payload of all changed files is >= 50 KB. Use MCP `push_files` only when total payload is < 50 KB, and only in a single batched call — never one file per call. Cargo.lock counts toward the total; a single large lockfile can exceed the threshold alone.
