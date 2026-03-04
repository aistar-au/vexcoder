---
name: vex-branch-contract
description: >
  Batch dispatch and branch-verification workflow for GitHub repos. Use this skill whenever the
  user wants to read raw/blob GitHub URLs, produce dispatch markdown with dependency gates and
  anchor tests, verify branch content through raw URLs or a .diff URL, generate a verification URL
  map, maintain the repo-wide raw URL index for newly added files, draft a PR motivation body, or
  run the end-to-end loop:
  read → dispatch → verify → push → raw-url-check → diff-check → merge.
---

# Vex Branch Contract Skill

An end-to-end skill for the **read → dispatch → verify → push → raw-url-check → diff-check → merge** loop used in Rust repo automation. Works with any locally-running coding agent.

---

## Overview of the Loop

```
Step 0  SYNC      Update local branch from remote before any verification/read
Step 1  READ      Fetch raw GitHub URL(s) from a branch or main
Step 2  DISPATCH  Write the batch dispatch prompt (markdown only, no plain text)
Step 3  VERIFY    Second-agent review of dispatch; apply corrections
Step 4  EXECUTE   Agent writes code, runs cargo test, pushes branch
Step 5  URL MAP   Generate /tmp/<branch>-verification-urls.md
Step 6  RAW CHECK Fetch every raw URL → HTTP 200 + content match
Step 7  DIFF CHECK Fetch .diff URL → confirm all expected paths present
Step 8  CI GREEN  clippy/rustfmt/tests + GitHub Actions pass
Step 9  MAP GATE  Update/check TASKS/completed/REPO-RAW-URL-MAP.md for new files
Step 10 MERGE     Merge commit (no squash, no rebase) → verify via raw map URL
```

Always output **pure markdown** when producing dispatch prompts or reports. Never emit plain prose paragraphs in dispatch output.

---

## Step 0 — Sync Local Before Verification

Before reading files, checking anchors, validating hunks, or producing gate
status, sync local state first:

```sh
git checkout main
git pull --ff-only
```

If the verification target is a non-`main` branch:

```sh
git fetch origin
git checkout <branch>
git pull --ff-only
```

Always report the head SHA used for verification.

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
git fetch origin
git checkout -b <branch-name> origin/main
```
> `git checkout main && git pull` is not sufficient if local `main` is behind.
> Branch directly from `origin/main`.

---

### Dependency graph

```
<ROOT> ✅
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

**Gate:** <PREV-TASK> anchor green [✅ if already done]

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

- Branch from `origin/main` using `git fetch origin && git checkout -b <branch> origin/main`.
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

1. `git fetch origin && git checkout -b <branch> origin/main`
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
7. Generate `/tmp/<safe-branch-name>-verification-urls.md` (see Step 5 script).

---

## Step 5 — Generate Verification URL Map

Run from repo root. Produces `/tmp/<branch>-verification-urls.md`.

```bash
bash .agents/skills/vex-branch-contract/scripts/gen_verification_urls.sh \
  -b <branch> \
  --base origin/main
```

The output file lists every changed file as a raw GitHub URL grouped by category (Cargo & Config / Source / Tests / Other). It also records commit SHA, ahead/behind counts, and a clean working-tree assertion.

**Manual equivalent** (if script unavailable):

```bash
git fetch origin
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
bash .agents/skills/vex-branch-contract/scripts/verify_raw_urls.sh \
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
bash .agents/skills/vex-branch-contract/scripts/verify_diff_url.sh \
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
- Resolve any conflicts with `git merge origin/main` on the feature branch, re-run anchor tests.
- Working tree must be clean (`git status --porcelain` → empty).

Do not create a PR until the branch is conflict-free and CI is green.

### Clippy failure playbook (common blockers)

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
   - Replace `contains_key` + `insert` with `entry(...).or_insert(...)`.
5. `clippy::manual_clamp`
   - Replace `.max(a).min(b)` with `.clamp(a, b)`.

---

## Step 9 — Repo Raw URL Map Gate

Project policy:

- `TASKS/TASKS-DISPATCH-MAP.md` is the descriptive dispatch contract document for this repo.
- `TASKS/completed/REPO-RAW-URL-MAP.md` is the canonical whole-repo raw URL file map.
- Keep this map synchronized with repository files.
- Update it on any drift (new/missing files, line-count drift, URL drift, header totals).

Check map coverage (required before push/PR):

```sh
bash .agents/skills/vex-branch-contract/scripts/update_repo_raw_url_map.sh --check
```

If the check reports drift, regenerate it:

```sh
bash .agents/skills/vex-branch-contract/scripts/update_repo_raw_url_map.sh
```

Then verify again:

```sh
bash .agents/skills/vex-branch-contract/scripts/update_repo_raw_url_map.sh --check
```

If no drift is present, update script prints a no-op message and leaves the file untouched.

---

## Step 10 — PR Body File and Merge

Generate the PR summary and write a markdown body file in `/tmp`:

```sh
bash .agents/skills/vex-branch-contract/scripts/branch_summary.sh \
  -b <branch> \
  --write-pr-body
# writes: /tmp/<safe-branch-name>-pr-body.md
```

Optional custom output path:

```sh
bash .agents/skills/vex-branch-contract/scripts/branch_summary.sh \
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
git merge --no-ff <branch> -m "Merge batch-<x>/adr-<nnn>: <short description>"
git push origin main
```

---

## Step 11 — Post-Merge Verification

After merge, re-fetch the original raw map URL (the TASKS or ADR document on main) and confirm:

- The dispatch tasks are addressed by files now on `main`.
- `git log --oneline -5` shows the merge commit.
- `cargo test --all-targets` is green on `main`.

---

## Scripts Reference

All scripts live in `.agents/skills/vex-branch-contract/scripts/`. Bootstrap them with:

```bash
mkdir -p .agents/skills/vex-branch-contract/scripts
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
| `-b / --branch <name>` | Branch to operate on (inferred from HEAD if omitted) |
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

1. **Always branch from `origin/main`** via `git fetch origin && git checkout -b <branch> origin/main`. Never from local `main`.
2. **Merge commit only** — `git merge --no-ff`. No squash. No rebase.
3. **Stop on red anchor** — never proceed to a dependent task if its gate is not green.
4. **`ENV_LOCK.blocking_lock()`** in all sync tests — `.lock().unwrap()` will not compile against `tokio::sync::Mutex`.
5. **Working tree must be clean** before any verification script runs.
6. **Only raw GitHub URLs** in agent prompts during Step 6. No full file content paste.
7. **All output is markdown** — no plain text paragraphs in dispatch or report documents.
8. **Repo map gate required** — run `update_repo_raw_url_map.sh --check`; if drift is reported, update then re-check.
9. **Final report required** — every batch must close with task results table, files changed, verification commands with exit codes, and open issues.
