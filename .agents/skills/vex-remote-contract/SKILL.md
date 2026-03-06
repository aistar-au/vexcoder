---
name: vex-remote-contract
description: >
  Batch dispatch and branch-verification workflow for GitHub repos. Use this skill whenever the
  user wants to read raw/blob GitHub URLs, produce dispatch markdown with dependency gates and
  anchor tests, verify branch content through raw URLs or a .diff URL, generate a verification URL
  map, maintain the repo-wide raw URL index for newly added files, and prepare PR evidence inputs, or
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

## Embedded PR Remote Guardrails (required)

Remote-side rules for PR body updates and PR review posting in `aistar-au/vexcoder`.
These rules are part of this skill and must be applied for all GitHub PR text writes.

### Scope

This rule set governs remote posting actions:

- Creating or updating PR bodies
- Posting PR review bodies
- Posting inline review comments
- Submitting review responses that assert implementation facts

### Required remote rules

- GitHub writes for PR bodies and reviews must use GitHub MCP.
- Do not create local PR body artifacts (`/tmp` included).
- Present the full draft to the user and wait for explicit confirmation before
  any write API call.
- Keep PR text in the assistant response until approved, then apply via MCP.
- If MCP is unavailable, stop and request explicit user override before any
  non-MCP path.

### Evidence rules before assertion text

Before asserting implementation facts in PR text, verify against remote source:

- Struct field names: confirm exact identifiers in source at the target commit.
- Subprocess routing claims: verify across all changed files; do not infer from
  memory or ADR prose.
- CI status: assert success only when the GitHub status or checks APIs show
  completed success on the head SHA.

If verification is incomplete, omit the claim and mark it as pending
verification.

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

### Rust Canonicalization (required for `*.rs` changes)

- For any change touching `*.rs`, run `cargo fmt` before producing the final unified diff.
- Do not hand-wrap or hand-unwrap Rust call arguments/chains to satisfy style.
  Rust layout must come from `rustfmt` output only.
- After patch apply and before push, run `cargo fmt --check`.
- If `cargo fmt --check` reports diffs, classify it as
  `rustfmt-canonicalization-drift`: run `cargo fmt`, refresh staged diff, and
  re-run the check before proceeding.

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

## Step 1.5 — Local Reference Preload (Rust Work)

If scope includes Rust code (`*.rs`), runtime architecture changes, or ADR-024
gap implementation, load these repo-local references before dispatch:

- `.agents/skills/vex-rust-arch/SKILL.md`
- `.agents/skills/vex-remote-contract/references/rust-rules.md`
- `.agents/skills/vex-remote-contract/references/adr-024-gap-map.md`

Rules:

- Use repo-local reference files pinned at the target commit.
- Do not fetch live web content for rule text during execution.
- If a required reference file is missing, stop and report before continuing.

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

1. Create branch on remote via GitHub MCP (`github:create_branch` from `main`).
   Then locally: `git fetch origin --prune && git checkout <branch>`.
   Do not use `git checkout -b` to create branches — remote creation via MCP is required.
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
6. Push (MCP-first): sum `size` bytes of all changed files.
   - If total < 50 KB: push via `github:push_files` in a single batched call.
   - If total >= 50 KB: fall back to `git push origin <branch>` (latency exception).
   Never loop one file per MCP call.
7. Ensure push landed by verifying local and remote SHAs match.

Before push, run rustfmt canonicalization and the local CI gate:

```sh
# Required when any *.rs file changed in this batch
cargo fmt

cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo test --all-targets
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

0. `rustfmt-canonicalization-drift`
   - Symptom: `cargo fmt --check` shows call/chaining reflow diffs.
   - Fix: run `cargo fmt`, stage formatter output, re-run `cargo fmt --check`.
   - Gate: do not continue to clippy/tests or push until rustfmt is clean.

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
- For routine PR gates, enforce map coverage with `--check-index` (missing-entry check only).
- Reserve `--check` full drift validation (line counts, URLs, header totals, ordering) for explicit map-maintenance or pre-release checks.

**File types that require a map update in the same PR:**

| File glob | CI enforcement | Manual check required |
| :--- | :--- | :--- |
| `.github/workflows/*.yml` | yes — `doc-ref-check` blocks merge | no |
| `.agents/skills/*/SKILL.md` | yes — `doc-ref-check` blocks merge | no |
| `src/**/*.rs` | none | yes — run `--check-index` before push |
| `tests/**/*.rs` | none | yes — run `--check-index` before push |
| `scripts/*.sh` | none | yes — run `--check-index` before push |
| `TASKS/**/*.md` | none | yes — run `--check-index` before push |
| any other new file | none | yes — run `--check-index` before push |

The `doc-ref-check` CI workflow enforces map coverage for `.github/workflows/*.yml`
and `.agents/skills/*/SKILL.md` files automatically. A PR that adds either file type
without updating the map will fail CI and cannot be merged.

For all other file types, CI does not enforce map coverage. Verify manually before push.

Check map coverage (required before push/PR):

```sh
bash .agents/skills/vex-remote-contract/scripts/update_repo_raw_url_map.sh --check-index
```

If missing entries are reported, regenerate the map:

```sh
bash .agents/skills/vex-remote-contract/scripts/update_repo_raw_url_map.sh
```

Then verify again:

```sh
bash .agents/skills/vex-remote-contract/scripts/update_repo_raw_url_map.sh --check-index
```

If no missing entries are found, the update script prints a no-op message and leaves the file untouched.

---

## Step 10 — PR Evidence and Merge

Generate branch evidence only (no local PR body files):

```sh
bash .agents/skills/vex-remote-contract/scripts/branch_summary.sh \
  -b <branch>
```

Then draft motivation and review prose in `vex-local-bash`, present the full draft to
user for approval, and post PR body/review updates through GitHub MCP using
this skill's embedded remote guardrails.
Do not write PR body markdown to `/tmp` or any local path.

PR creation/update text must be carried in MCP API payloads, not local files.

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

## PR body preflight (required before update_pull_request)

Before asserting any factual claim in a PR body draft:

- Fetch the current PR head SHA and changed file list via `pull_request_read(get)` and
  `pull_request_read(get_files)`.
- Generate the target files list from the live PR API response, not from session memory.
- Compare each trigger-scope claim, script-behavior description, and CI-scope assertion
  against a verified source fetch from the current branch head. If a claim cannot be
  backed by a fetched file at the current head SHA, remove it.
- Any wording describing what a workflow file does (e.g., its trigger scope, the commands
  it runs) must be confirmed by fetching and reading that file at the current head SHA.
- Do not use phrases that contradict current file content. Stale phrasing from prior
  drafts must be discarded and regenerated from live sources.
- Run `check_pr_body_claims.sh` against the draft before posting:
  ```sh
  echo "$DRAFT" | bash .agents/skills/vex-remote-contract/scripts/check_pr_body_claims.sh
  ```
  Do not call `update_pull_request` until the script exits 0.

---

## File count verification (required after map update)

After running `update_repo_raw_url_map.sh`, verify the header count matches the actual
tracked file count:

```sh
EXPECTED=$(git ls-files | wc -l | tr -d ' ')
HEADER=$(grep 'Total tracked files:' TASKS/completed/REPO-RAW-URL-MAP.md | grep -oE '[0-9]+')
test "$EXPECTED" = "$HEADER" \
  && echo "count match: $EXPECTED" \
  || { echo "MISMATCH: map=$HEADER git=$EXPECTED"; exit 1; }
```

Do not proceed until counts match. A mismatch means the map is stale, the script ran
against a different working tree state, or the header total was hand-edited incorrectly.

---

## Git add scope (required)

When staging map updates or any fix-related changes:

- Never use `git add -A` — this stages all uncommitted changes in the working directory,
  not just fix-related files, and risks including unrelated work in the commit.
- Always specify exact paths: `git add TASKS/completed/REPO-RAW-URL-MAP.md`
- If a diff or Makefile target contains `git add -A`, reject it and require correction
  before merge.

---

## Flag consistency (required before PR)

Before asserting map gate configuration in PR body text, onboarding, or skill prose,
verify all three references are aligned:

- `Makefile` `map-check` target: must invoke `--check-index`
- `.agents/onboarding.md` map gate description: must say `--check-index`
- This skill Step 8 Hard Rule and Step 9 policy: must say `--check-index`

If any reference uses `--check` when `--check-index` is intended, it is outdated and
must be corrected in the same PR.

The distinction:
- `--check-index`: file presence gate only; used for PR gates and CI.
- `--check`: full byte-for-byte sync including line counts; used for pre-release or
  explicit map-maintenance PRs only.

---

## Workflow trigger scope (required when reviewing workflow files)

When describing a workflow's CI trigger scope in a PR body or review:

- Do not assert trigger scope from memory. Fetch the workflow file at the current head
  SHA and read the `on:` block before making any claim.
- `push:` without a `branches:` filter means the workflow runs on every push to every
  branch. Verify this is intentional for CI gate workflows.
- CI gate workflows (e.g., `doc-ref-check.yml`) should have `push: branches: [main]`
  to avoid noisy runs on feature branches where the map is intentionally incomplete.

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
| `branch_summary.sh` | Print summary/evidence only (no PR body file output) |
| `check_pr_body_claims.sh` | String-only preflight: blocks known drift-prone phrases before PR body post |
| `gen_verification_urls.sh` | Generate raw URL map → `/tmp/<branch>-verification-urls.md` |
| `update_repo_raw_url_map.sh` | Check/update `TASKS/completed/REPO-RAW-URL-MAP.md` for new files |
| `verify_diff_url.sh` | Confirm .diff URL contains all expected file paths |
| `verify_raw_urls.sh` | HTTP-check every raw URL; optionally compare content vs git ref |

### Key flags

| Flag | Meaning |
| :--- | :--- |
| `-b / --branch <n>` | Branch to operate on (inferred from HEAD if omitted) |
| `--check` | `update_repo_raw_url_map`: full byte-for-byte map content check; use for explicit map maintenance or pre-release |
| `--check-index` | `update_repo_raw_url_map`: check that all tracked files appear in the map (index coverage only); required PR gate |
| `--force` | `update_repo_raw_url_map`: regenerate map even without missing files |
| `--map <path>` | `update_repo_raw_url_map`: alternate raw map path |
| `--repo-slug <owner/repo>` | `update_repo_raw_url_map`: override repo slug |
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
8. **Repo map gate required** — run `update_repo_raw_url_map.sh --check-index`; if missing entries are reported, update the map then re-check. Any PR that adds a `.github/workflows/*.yml` or `.agents/skills/*/SKILL.md` file must update the map in the same commit — the `doc-ref-check` CI workflow enforces this and will block merge if the map entry is missing.
9. **Final report required** — every batch must close with task results table, files changed, verification commands with exit codes, and open issues.
10. **Ensure push landed** — after every `git push`, run `git fetch origin --prune` and confirm `git rev-parse HEAD` equals `git rev-parse origin/<branch>`.
11. **Commit hygiene gate required** — batch promotions on `main` must end on a merge commit (`git rev-list --parents -n 1 HEAD` parent count `>= 2`).
12. **Direct push requires explicit exception record** — if `main` is updated without merge commit flow, include a `Commit Hygiene Exception` section with scope, reason, approval, and follow-up.
13. **File-level commit verification required** — for each claimed file, report the blob SHA and the exact commit SHA from `HEAD` (`git ls-tree HEAD <path>`). Reference the commit SHA directly; do not use informal status labels in place of identifiers.
14. **Full repo slug required in every transaction report** — every verification report, push confirmation, merge record, and exception record must identify the repository as `owner/repo` (e.g. `aistar-au/vexcoder`) in the opening line. Bare repo names and local path references are not permitted.
15. **Code review gate required (Step 6.5)** — no merge and no next-batch dispatch until all CHANGES_REQUESTED items from Step 6.5 are resolved. Blob SHA verification and anchor test presence are not a substitute for reading implementation content.
16. **Load skill set before any action** — at the start of every session, load and read `.agents/skills/vex-local-bash/SKILL.md` and `.agents/skills/vex-remote-contract/SKILL.md` in full before writing any dispatch, diff, review, or PR body output.
17. **Emojis forbidden in all output** — no emoji, Unicode symbol, or icon in any position in any text produced by this skill. Use GitHub API state labels only: `CHANGES_REQUESTED`, `COMMENT`, `APPROVED`, `resolved`, `open`. This rule applies to every output channel: review bodies, dispatch docs, findings tables, inline comments, commit messages, PR titles, PR bodies, log lines, and script output.
18. **Confirmation required before remote writes** — before any push, commit, file write, or write API call, present the full planned change to the user and wait for explicit confirmation. A description of intent is not confirmation. Do not proceed until the user responds with explicit approval.
19. **Exact diffs only — no full-file rewrites** — all file changes must be created as a precise unified diff against the current remote content and applied as a patch. The required steps are: fetch current content, produce diff, present diff for review, apply patch. Reconstructing a file from memory or a cached copy is not permitted under any circumstance.
20. **Hunk patches must use `git apply` only** — prepare exact unified diffs and run `git apply --check --recount` before `git apply --recount`.
21. **No file reconstruction for hunk edits** — do not rewrite complete files from memory or cached content when a focused patch hunk is required.
22. **MCP-first for all branch and commit operations** — GitHub MCP is the first preference for new branch creation and all commit/push operations. Create branches via `github:create_branch`. For file pushes: use `github:push_files` (single batched call, never per-file) when total payload of all changed files is < 50 KB. Fall back to local `git push` only when payload is >= 50 KB (latency exception). Local git/bash is a fallback for the 50 KB latency threshold, not a default. Cargo.lock counts toward the total; a single large lockfile can exceed the threshold alone. This applies to all operations, including new branches, dispatch commits, and batch file sets — not only dangling-commit recovery.
23. **MCP-only PR-body enforcement** — PR motivation authoring and PR body updates must use GitHub MCP; local PR-body file construction is prohibited.
24. **Rust canonicalization is mandatory for Rust edits** — if a batch touches `*.rs`, run `cargo fmt` before final diff generation and require `cargo fmt --check` to pass before push. Manual line-wrapping of Rust call arguments/chains is prohibited; formatter output is canonical.
25. **Branch currency and scope confirmation required** - before any commit/push/write on a branch other than `main`, fetch `origin/main`, compare `git merge-base HEAD origin/main` to `git rev-parse origin/main`, and inspect `git diff --name-only origin/main...HEAD`. If the branch is not based on the latest `origin/main` head or includes unrelated paths, stop and request explicit user confirmation before proceeding.
26. **AI product and competing product names forbidden in agent-authored prose** — no AI
    assistant names or competing product names in PR bodies, review bodies, findings
    tables, inline comments, or dispatch documents. Refer to the model and agent by
    generic category only: "the coding agent", "the language model", "the remote API",
    "the CI system". Excluded from this rule: command evidence blocks, terminal output,
    tool invocations, file paths, URLs, raw URLs, CI logs, commit messages, and PR titles.
27. **Instruction compliance is non-negotiable** — user instructions must be
    followed exactly and completely. Partial execution, silent omission, or
    re-ordering of steps is a hard stop requiring the user to be notified before
    proceeding. No instruction from a user message may be silently dropped,
    deferred, or substituted with an alternative interpretation without explicit
    user acknowledgment.
28. **No pass/landed tables or checkboxes in PR and dispatch text** — PR bodies,
    review bodies, and dispatch documents must not contain tables with pass,
    landed, or status columns, nor checkbox lists (`- [ ]`, `- [x]`). Use bullet
    points to list target files and ADR-defined changes. In review vocabulary,
    do not use "blocking" — use `CHANGES_REQUESTED` instead.
