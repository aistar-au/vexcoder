# Contributing to vexcoder

> **Version:** This workflow applies from `v0.1.0-rc.1` onward.
> **Architecture decisions** are placed in [`adr/`](adr/ADR-README.md).
> The ADRs explain *why* the project is structured this way. Read them before opening a PR.
>
> **Agent bootstrap:** repo-local product guidance stays here, but the active
> local skills are now located in the internal private repo
> `../vexdraft/.agents/skills/`.
> Read [`AGENTS.md`](AGENTS.md) first for the dependency map and required load
> order before using the private skill tree against this repo.

---

## The Agentic Workflow (Test-Driven Manifest)

`vexcoder` uses the **Test-Driven Manifest (TDM)** strategy for all bug fixes, features, and refactors. The full rationale is in [ADR-001](adr/completed/ADR-001-tdm-agentic-manifest-strategy.md). The short version:

1. **Identify task** — Check `adr/` for open architecture decisions.
2. **Anchor test** — Every task has exactly one failing Rust test before work begins. No anchor, no dispatch.
3. **Module isolation** — Work is confined to the `Target File` named in the task manifest (± one helper file).
4. **Verification** — Success is `cargo test <anchor_name>` passing, `cargo nextest run` staying green for the branch, plus `cargo test --all-targets` showing no regressions.

Runtime mode additions and naming-policy changes require explicit confirmation before implementation or documentation. See ADR-007.
Canonical production dispatch is runtime-core only: `Runtime<M>::run` → `RuntimeMode::on_user_input` → `RuntimeContext::start_turn`.
Alternate app-owned dispatch channels are forbidden in production paths.
Runtime-core ratatui TUI behavior must conform to ADR-009, ADR-010, and ADR-011 before merge.
Runtime-core TUI deployment is gated by ADR-012; no deploy if any ADR-012 item is unmet.
Architecture gates enforcing ADR-007 must remain green:
`bash scripts/check_no_alternate_routing.sh`
`bash scripts/check_forbidden_imports.sh`
Tests that mutate process environment variables must hold `crate::test_support::ENV_LOCK`; `cargo test --all-targets` must pass without `--test-threads=1`. Keep `.git/hooks/pre-push` installed and wired to `cargo nextest run` for every local push so nextest uses its default cross-platform concurrency. The `ci` workflow runs 8 parallel jobs (lint, clippy, nextest, doctest, test-all-targets on Ubuntu; clippy+fmt, test, package on Windows) with cargo registry and build-artifact caching.

---

## Planning and Audit-Only Requests

Planning-only and audit-only requests are strictly no-touch by default:
no file create, edit, rename, move, or delete is allowed during a planning/audit-only pass.

If the user later asks to implement changes in the same session, switch to edit mode only
after explicit user confirmation.

Use the same explicit-confirmation standard already required for runtime mode additions and
naming-policy changes.

---

## Language and PR Hygiene

Use neutral repository-approved status terms in docs, ADRs, plans, PR text, and
agent-authored notes. Keep `bash scripts/check_forbidden_names.sh` green and
treat that gate as authoritative for wording that must not ship.

When describing the operator surface in prose, prefer `CLI`, `CLI app`, or
`surface` over the generic host-app noun (banned, see `check_forbidden_names.sh`)
unless the exact technical term is required for a crate name, ANSI control
sequence, TTY-size API, or quoted log output.

Every PR body uses five sections in this order: `Summary`, `Motivation`,
`Approach`, `Validation`, and `Risks`. Do not omit `Risks`, even for docs-only
or small cleanup lanes.

---

## Task Naming Convention

| Prefix | Type | Example |
| :--- | :--- | :--- |
| `CRIT-XX` | Critical bug | `CRIT-02-serde-fix.md` |
| `FEAT-XX` | Feature | `FEAT-01-streaming-ui.md` |
| `REF-XX` | Refactor | `REF-02-runtime-contract.md` |
| `SEC-XX` | Security | `SEC-01-path-security.md` |
| `CORE-XX` | Core infrastructure | `CORE-01-sse-parser.md` |
| `DOC-XX` | Documentation | `DOC-01-api-docs.md` |

---

## Rust Module File Naming (Rust 2018+)

Use path-based module entry files across `src/`.

| Situation | Required path |
| :--- | :--- |
| Top-level module entry | `src/<module>.rs` |
| Nested module | `src/<module>/<submodule>.rs` |

Do not introduce new `src/*/mod.rs` files for production modules unless an
external tool or macro requires that layout. Test-only module directories such
as `src/app/tests/mod.rs` and `src/state/conversation/tests/mod.rs` are allowed
when they are used only to split oversized test suites.

---

## Runtime-core Status

REF-08 full cutover is complete and merged (2026-02-19).
Canonical dispatch and layering rules are now governed by ADR-007 and ADR-008.

---

## Quick Start

```bash
# 1. Install Rust (stable toolchain required)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# 1a. Install the pre-push runner used by the local hook
cargo install cargo-nextest --locked

# 2. Verify the environment
cargo nextest run
cargo test --all-targets

# 3. Read the relevant ADR in adr/, identify the anchor test

# 4. Implement, then verify
cargo test test_crit_XX_anchor_name -- --nocapture
cargo nextest run

# 5. Confirm no regressions
cargo nextest run
cargo test --all-targets
bash scripts/check_no_alternate_routing.sh
bash scripts/check_forbidden_imports.sh
bash scripts/check_forbidden_names.sh
```

---

## Release Packaging

Package release changes on a review branch first, verify them locally, and open the PR without waiting on a duplicate packaging workflow run.

```bash
git switch -c work/v<current-version>-packaging
make gate
make release TARGET=x86_64-unknown-linux-gnu
git push -u origin work/v<current-version>-packaging
```

On Windows PowerShell 7, use the native packaging script instead of `make release`:

```powershell
git switch -c work/v<current-version>-packaging
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo nextest run
cargo test --all-targets
.\scripts\release.ps1 -Target x86_64-pc-windows-msvc
git push -u origin work/v<current-version>-packaging
```

Windows packaging is currently an unsigned alpha path. Platform trust warnings are expected until code signing lands; evaluate a compatible signing service only when the packaging ADR set explicitly requires it.

The packaging scripts derive the archive tag from `Cargo.toml` and reject mismatched tag inputs. `.github/workflows/release.yml` now runs only for tag pushes and manual dispatch so review branches do not duplicate the main PR checks. After the branch gates are green and the local packaging smoke checks look correct, open the PR. Publish the prerelease only after the merge commit is on `main`:

```bash
git switch main
git pull --ff-only origin main
git tag -a v<current-version> -m "Release v<current-version>"
git push origin v<current-version>
```

The tag push is a local post-merge release step. Do not open a second PR patch
just to publish the matching `v<current-version>` tag.

For semver and short-SHA tags, the pushed tag drives the rest of the release
flow automatically: archive packaging, checksums, signature bundles, the
release entry, and a generated `CHANGELOG-<tag>.md` asset all publish from the
same tag event.

A daily GitHub-hosted schedule (`.github/workflows/nightly.yml`) creates
a short-SHA snapshot tag from the current HEAD of `main` if one does
not already exist. The tag push triggers the release workflow to produce
a nightly pre-release build.
For immediate snapshot releases, the operator can manually create a
short-SHA tag after merge. See `RELEASING.md` for the full tag format
table and manual tagging instructions.

Do not merge packaging work directly from a local debug session; keep the review and merge step explicit.

### Automated version bump

Use the `version-bump` workflow dispatch to bump the version from the
browser: Actions > version-bump > Run workflow > enter new version. The
workflow runs `scripts/bump-version.sh`, commits, and opens a PR. See
`RELEASING.md` for the full automated flow.

---

## Remote Agent Sessions

Repository-level background sessions are self-contained. They must not load or
depend on the private `vexdraft` skill tree. Use the checked-in background
session contract under `.github/instructions/` and the repository-hosted agent
instructions file under `.github/`.

- In `AGENTS.md`, hosted sessions must ignore the `Local bootstrap only`
  section and every `../vexdraft` reference.
- Hosted sessions must not read any `SKILL.md` file.
- Hosted sessions must use English only in agent-authored output.
- Hosted sessions must use text-only verification and reporting. Do not create
  screenshots, screen captures, pseudo-screenshots, parsed cli snapshots,
  image artifacts, or temporary visual-surrogate files.
- Hosted sessions must not create ad hoc temporary projects or files whose only
  purpose is to simulate, capture, or restyle the UI for visual verification.
- The setup workflow validates the hosted-session contract and must stay
  self-contained. It must not clone `vexdraft`, copy private skills into the
  background-session home directory, or depend on platform secrets just to make
  the agent start.
- Repository-wide background-session guidance is found under
  `.github/instructions/`, the repository-hosted agent instructions file under
  `.github/`, and the custom agent profiles.
- The setup workflow only affects background sessions after it lands on the
  default branch. Manual workflow dispatch is still useful for validating the
  hosted-session bootstrap contract on a feature branch before merge.
- The repository-level agent profile follows the branch you target. Use a
  review branch as the `--base` argument when you want the remote session
  to see branch-local agent changes.
- New or renamed custom agent profile files are only selectable through
  `gh agent-task --custom-agent` after they exist on the default branch.
  Branch-local profiles remain useful for repository content and follow-up
  promotion work, but the remote agent catalog itself is resolved from the
  default-branch profile set before the session starts.
- Keep the preferred model pinned inside the agent profile itself rather than
  passing a model flag in `gh agent-task create`. If the hosting surface does
  not honor the profile pin, record the observed behavior in the session log
  instead of changing invocation style.
- If `rg` is unavailable in the hosted runner, fall back to `git grep -n`,
  `grep -RIn`, or direct file reads and continue.
- In hosted agent-authored prose, explicitly avoid vendor and proprietary
  assistant names unless a path, URL, command, or quoted log line requires the
  exact string. When possible, rewrite them as `the hosted coding agent`, `the
  profile-pinned model`, `the proprietary reference`, `the automated
  reviewer`, or `the hosted runtime`.
- If a hosted-run validation step fails only because the runner lacks a local
  CLI that the repository does not provision, record the gap as environment
  drift instead of installing ad hoc tooling inside the session.
- For hosted sessions that only touch docs, instructions, agent profiles, or
  workflows, run `cargo fmt --check`, `cargo test --all-targets`, and
  `bash scripts/check_forbidden_names.sh` first. Run `make gate-fast` only if
  the runner image already has the required gate tools.
- For one feature lane, keep one authoritative integration branch and one
  final PR to `main`. Parallel hosted shard PRs are allowed only when each
  shard has an explicit disjoint write set and all accepted commits are
  promoted onto the shared integration branch before merge.
- For any remote code-bearing lane, create or reuse the draft PR before the
  first code-bearing push. Do not wait for a later merge prompt to open the
  PR.
- After every code-bearing commit or patch set, push immediately, fetch
  `origin`, and verify `HEAD` matches `origin/<branch>`. Do not continue from
  unpublished local-only branch state.
- Once the remote lane exists, treat the remote branch head and PR state as
  authoritative for commit-debug, CI watch, PR body updates, review cleanup,
  and merge readiness.
- Every `gh agent-task create` invocation must be followed immediately by an
  explicit log tail with `gh agent-task view <session-id> --log --follow`.
  Treat log tailing and violation triage as part of launch, not as an optional
  post-launch observation step.
- Do not continue to `gh pr view`, `gh pr checks`, promotion, or merge work
  until that paired launch-log tail has completed and any contract violation
  has been handled.

Authoritative launch suffix for hosted prompts:

- Use English only.
- Do not read any `SKILL.md` file.
- Do not bootstrap, inspect, or depend on private skills or adjacent repos.
- Use text-only verification only.
- Do not create screenshots, screen captures, pseudo-screenshots, parsed
  cli snapshots, image artifacts, or temporary visual-surrogate files.
- Do not create ad hoc temporary projects or files whose only purpose is to
  simulate, capture, or restyle the UI for visual verification.

Available profiles:

- `vexcoder-ui-parity-orchestrator` for prompt interactivity, slash commands,
  `@file` expansion, startup API/model prompting, editor behavior, and final
  integration/conflict cleanup.
- `vexcoder-ui-paragraph-renderer` for the direct ANSI surface controller,
  fullscreen paragraph rendering, star/cosmic accent styling, prompt-dock
  drawing, and transcript regression coverage.
- `vexcoder-transcript-renderer-overhaul` for task-state layout logic,
  fallback-renderer parity, fixed-height prompt geometry, blank-initial
  transcript behavior, and related layout/test contracts.
- `rust-change-auditor` for review, regression diagnosis, and post-merge
  audit of Rust changes across the codebase.

### Parallel UI overhaul pattern

Use this pattern when the UI lane is broad enough to justify concurrent
repository-hosted sessions.

1. Create and push one shared integration branch from the latest `origin/main`.
2. Launch one hosted shard per disjoint write set from that shared base
   branch.
3. Require every hosted shard to report its base SHA, owned files,
   code-bearing commit SHAs, and changed paths before promotion.
4. Promote accepted shard commits onto the shared integration branch by
   cherry-pick. Open only one final PR from that branch to `main`.
5. If `main` moves during execution, refresh the shared integration branch
   from the latest `origin/main`, cherry-pick finished shard commits onto it,
   and relaunch only the shards whose owned files were invalidated.

Recommended shard ownership for UI-overhaul work:

- `vexcoder-ui-parity-orchestrator`
  `src/app.rs`, `src/app/queries.rs`, `src/app/commands/mod.rs`,
  `src/app/input.rs`, `src/app/inline.rs`, `src/app/model_update.rs`,
  `src/app/turn.rs`, `src/bin/vex.rs`, `src/ui/editor/mod.rs`, and final
  workflow/doc cleanup only when explicitly assigned.
- `vexcoder-ui-paragraph-renderer`
  `src/ui/render/mod.rs`, `src/ui/render/transcript.rs`,
  `src/ui/render/tests.rs`, and closely related transcript helpers.
- `vexcoder-transcript-renderer-overhaul`
  `src/app/layout.rs`, `src/app/tests/`, `src/ui/render/mod.rs`,
  `src/ui/layout.rs`,
  `tests/layout_underflow_tests.rs`, and related layout/timeline helpers.

Example launch sequence:

```bash
git fetch origin --prune
git switch -c work/vexcoder-ui-overhaul origin/main
git push -u origin work/vexcoder-ui-overhaul

gh agent-task create \
  --base work/vexcoder-ui-overhaul \
  --custom-agent vexcoder-ui-parity-orchestrator \
  "Shard: prompt interactivity. Own only src/app.rs, src/app/queries.rs, src/app/commands/mod.rs, src/app/input.rs, src/app/inline.rs, src/app/model_update.rs, src/app/turn.rs, src/bin/vex.rs, and src/ui/editor/mod.rs. Focus on prompt submission, slash commands, @file expansion, and startup API/model prompting. Do not edit layout or ANSI-surface files. Report base SHA, changed paths, and code-bearing commit SHAs before stopping. Use English only. Do not read any SKILL.md file. Do not bootstrap, inspect, or depend on private skills or adjacent repos. Use text-only verification only. Do not create screenshots, screen captures, pseudo-screenshots, parsed cli snapshots, image artifacts, or temporary visual-surrogate files. Do not create ad hoc temporary projects or files whose only purpose is to simulate, capture, or restyle the UI for visual verification."
gh agent-task view <session-id-from-create-output> --log --follow

gh agent-task create \
  --base work/vexcoder-ui-overhaul \
  --custom-agent vexcoder-ui-paragraph-renderer \
  "Shard: ratatui fullscreen surface. Own only src/ui/render/mod.rs, src/ui/render/transcript.rs, src/ui/render/tests.rs, and transcript-local helpers. Focus on paragraph rendering, star/cosmic accents, prompt-dock drawing, and removal of stray top-surface chrome. Do not edit app layout files. Report base SHA, changed paths, and code-bearing commit SHAs before stopping. Use English only. Do not read any SKILL.md file. Do not bootstrap, inspect, or depend on private skills or adjacent repos. Use text-only verification only. Do not create screenshots, screen captures, pseudo-screenshots, parsed cli snapshots, image artifacts, or temporary visual-surrogate files. Do not create ad hoc temporary projects or files whose only purpose is to simulate, capture, or restyle the UI for visual verification."
gh agent-task view <session-id-from-create-output> --log --follow

gh agent-task create \
  --base work/vexcoder-ui-overhaul \
  --custom-agent vexcoder-transcript-renderer-overhaul \
  "Shard: task-state layout and ratatui renderer. Own only src/app/layout.rs, src/app/tests/, src/ui/render/mod.rs, src/ui/layout.rs, tests/layout_underflow_tests.rs, and directly related helpers. Focus on single-stream transcript layout, fixed 3-line prompt geometry, blank initial transcript behavior, and renderer parity. Do not edit ANSI transcript files. Report base SHA, changed paths, and code-bearing commit SHAs before stopping. Use English only. Do not read any SKILL.md file. Do not bootstrap, inspect, or depend on private skills or adjacent repos. Use text-only verification only. Do not create screenshots, screen captures, pseudo-screenshots, parsed cli snapshots, image artifacts, or temporary visual-surrogate files. Do not create ad hoc temporary projects or files whose only purpose is to simulate, capture, or restyle the UI for visual verification."
gh agent-task view <session-id-from-create-output> --log --follow
```

Start a UI parity session from the GitHub CLI with:

```bash
gh agent-task create \
  --base <review-branch> \
  --custom-agent vexcoder-ui-parity-orchestrator \
  "Investigate prompt interactivity, slash commands, @file expansion, startup API/model prompting, and stale docs. Use English only. Do not read any SKILL.md file. Do not bootstrap, inspect, or depend on private skills or adjacent repos. Use text-only verification only. Do not create screenshots, screen captures, pseudo-screenshots, parsed cli snapshots, image artifacts, or temporary visual-surrogate files. Do not create ad hoc temporary projects or files whose only purpose is to simulate, capture, or restyle the UI for visual verification."
gh agent-task view <session-id-from-create-output> --log --follow
```

Tail an existing session with:

```bash
gh agent-task view <session-id> --log --follow
```

Prefer the unique session id when multiple hosted runs are active. List the
sessions first when the identifier is unknown:

```bash
gh agent-task list
```

Start a paragraph-rendering session with:

```bash
gh agent-task create \
  --base <review-branch> \
  --custom-agent vexcoder-ui-paragraph-renderer \
  "Investigate the ratatui fullscreen surface, paragraph rendering, star/cosmic accent styling, prompt-dock drawing, and stale docs. Use English only. Do not read any SKILL.md file. Do not bootstrap, inspect, or depend on private skills or adjacent repos. Use text-only verification only. Do not create screenshots, screen captures, pseudo-screenshots, parsed cli snapshots, image artifacts, or temporary visual-surrogate files. Do not create ad hoc temporary projects or files whose only purpose is to simulate, capture, or restyle the UI for visual verification."
gh agent-task view <session-id-from-create-output> --log --follow
```

Start a timeline/fallback-renderer session with:

```bash
gh agent-task create \
  --base <review-branch> \
  --custom-agent vexcoder-transcript-renderer-overhaul \
  "Investigate task-state layout logic, ratatui renderer parity, fixed prompt geometry, blank-initial transcript behavior, and stale docs. Use English only. Do not read any SKILL.md file. Do not bootstrap, inspect, or depend on private skills or adjacent repos. Use text-only verification only. Do not create screenshots, screen captures, pseudo-screenshots, parsed cli snapshots, image artifacts, or temporary visual-surrogate files. Do not create ad hoc temporary projects or files whose only purpose is to simulate, capture, or restyle the UI for visual verification."
gh agent-task view <session-id-from-create-output> --log --follow
```

### Post-session workflow (mandatory steps A–H)

After an agent session completes, the operator must follow these steps in
order.

1. **A — Tail and debug logs**: identify each concurrent session by its unique
   session ID immediately after launch and use
   `gh agent-task view <session-id> --log --follow`. If the logs show private
   skill bootstrap attempts, `SKILL.md` reads, non-English output, screenshot
   or pseudo-screenshot plans, temporary visual artifacts, or ad hoc tool
   installation, stop the run, record the violation, correct the prompt or
   profile, and relaunch before promotion.
2. **B — Create review branch**: create a `work/<topic>` branch
   from `origin/main` and cherry-pick the agent's commits.
   Inspect the hosted PR first with
   `gh pr view <pr> --json headRefName,commits,statusCheckRollup`.
   If the hosted PR has only a planning commit or no file diff, treat it as
   draft-only evidence and do not present the change as implemented.
   For parallel shards, cherry-pick them onto one shared integration branch in
   a deterministic order, then resolve cross-shard conflicts there rather than
   reopening multiple competing PRs to `main`.
3. **C — Commit-debug loop**: run `vexdraft/scripts/commit-debug.py`, fix
  findings, push after every code-bearing fix, verify the remote head SHA,
  and re-run until `PASS`.
  Keep `.git/hooks/pre-push` active so each push re-runs `cargo nextest run`
  before the remote review cycle.
  Before each push from the review branch, run `git fetch origin --prune && git rebase origin/main`
  so the branch is rebased onto the latest moving mainline.
4. **D — Hide bot comments**: minimize automated reviewer bot comments via
   GraphQL `minimizeComment` with `OUTDATED` classifier.
5. **E — Sanitize brand names**: scan PR body, commit messages, and comments
   for proprietary brand names before posting.
6. **F — Watch CI**: keep the PR in draft, monitor all checks with
  `gh pr checks --watch`, and fix any failures before merge.
7. **G — Update documentation**: refresh CONTRIBUTING, architecture docs,
   commands docs, and the raw URL map for all changed files.
8. **H — Handle main drift**: if `origin/main` advanced during the hosted
   batch, refresh the shared integration branch from the new main head, repeat
   the cherry-pick sequence there, and relaunch only the shards whose owned
   files were invalidated by upstream changes.

---

## Project Structure

```
~/git-repo/
├── vexcoder/               # This repo — product code and release CI only
│   ├── CONTRIBUTING.md
│   ├── README.md
│   ├── adr/           # Architecture Decision Records
│   ├── src/                # Rust crate source
│   └── tests/              # Integration tests
└── vexdraft/               # Adjacent devops repo — local operator, commit-debug, skills
    └── scripts/
        └── commit-debug.py # Multi-provider pre-push reviewer (called by operator)
```

`vexdraft` must exist at `../vexdraft` relative to this repo for the operator
loop and pre-push review to function. The internal layout is the assumed path contract.

```
vexcoder/ (standalone view)
├── CONTRIBUTING.md                # Workflow guide + source map
├── README.md                      # Runtime and quickstart
├── adr/                      # Architecture Decision Records (open + completed)
├── src/                           # Rust crate source
│   └── bin/vex.rs                 # Binary entrypoint
└── tests/                         # Integration tests
```

---

## Tracked Rust Source Map (`*.rs`)

| File | Short description (with raw URL) |
| :--- | :--- |
| `src/lib.rs` | Crate root exporting runtime/app/api/state/tools/ui modules. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/lib.rs> |
| `src/bin/vex.rs` | Production binary entrypoint and managed TUI startup loop. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/bin/vex.rs> |
| `src/api.rs` | API module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/api.rs> |
| `src/api/client/mod.rs` | HTTP client module root for protocol selection, request/stream setup, and shared payload wiring; tool schemas now live under `src/api/client/tools.rs`. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/api/client/mod.rs> |
| `src/api/logging.rs` | Shared API debug/error logger and env-based log path handling. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/api/logging.rs> |
| `src/api/mock_client.rs` | Mock streaming client used by tests. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/api/mock_client.rs> |
| `src/api/stream.rs` | Stream/SSE event parsing helpers used by API layer. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/api/stream.rs> |
| `src/app.rs` | Current interactive application module root: TUI mode state, input, overlays, history, and runtime-facing coordination. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app.rs> |
| `src/app/queries.rs` | TuiMode read-only query methods and computed properties extracted from app facade under ADR-028 phase 4. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/queries.rs> |
| `src/app/commands/mod.rs` | Slash-command handler module root; command families are split across focused files under `src/app/commands/`. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/commands/mod.rs> |
| `src/app/ctor.rs` | TuiMode construction methods extracted from app facade under ADR-028 phase 4. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/ctor.rs> |
| `src/app/errors.rs` | AppError wrapper type and AppResult type alias for error handling. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/errors.rs> |
| `src/app/facade.rs` | API client facade builder with project instructions and bootstrap configuration. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/facade.rs> |
| `src/app/inline.rs` | Inline `@`-token file-expansion methods extracted from app facade under ADR-028 phase 3. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/inline.rs> |
| `src/app/input.rs` | User-input and interrupt handler methods extracted from app facade under ADR-028 phase 5. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/input.rs> |
| `src/app/layout.rs` | Layout-state and command-routing helper methods extracted from app facade under ADR-028 phase 4. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/layout.rs> |
| `src/app/model_update.rs` | Model-update handler methods extracted from app facade under ADR-028 phase 5. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/model_update.rs> |
| `src/app/overlay.rs` | Overlay and approval handler methods extracted from app facade under ADR-028 phase 2. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/overlay.rs> |
| `src/app/runtime_build.rs` | Runtime-construction functions `build_runtime` and `build_runtime_with_resume` extracted from app facade under ADR-028 phase 6. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/runtime_build.rs> |
| `src/app/scroll.rs` | Viewport and history scroll methods extracted from app facade under ADR-028 phase 2. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/scroll.rs> |
| `src/app/shell.rs` | Bang-command approval and command-session spawn methods extracted from app facade under ADR-028 phase 3. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/shell.rs> |
| `src/app/tests/mod.rs` | App-level unit and integration tests module root extracted from app facade under ADR-028 phase 1. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/tests/mod.rs> |
| `src/app/turn.rs` | Turn-lifecycle and command-session tracking methods extracted from app facade under ADR-028 phase 2. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/turn.rs> |
| `src/app/turn_start.rs` | Turn-dispatch and context-assembly helper methods extracted from app facade under ADR-028 phase 3. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/turn_start.rs> |
| `src/app/util.rs` | Module-level helper functions extracted from app facade under ADR-028 phase 1. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/app/util.rs> |
| `src/batch_mode.rs` | Non-interactive batch runner for `vex exec`, including JSONL and text turn output. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/batch_mode.rs> |
| `src/config.rs` | Layered config loading and validation across environment, repo-local, user, and system sources. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/config.rs> |
| `src/custom_commands.rs` | Custom user-defined commands loaded from `.vex/commands` directory. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/custom_commands.rs> |
| `src/doctor.rs` | Diagnostic checks for system configuration and health status. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/doctor.rs> |
| `src/edit_diff.rs` | Edit preview diff/hunk formatting utilities. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/edit_diff.rs> |
| `src/export.rs` | Task execution record export in JSONL or Markdown format. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/export.rs> |
| `src/git_hooks.rs` | Git hook install/remove helpers and commit-trailer hook script. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/git_hooks.rs> |
| `src/local_api.rs` | HTTP server implementation for local API with axum web framework. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/local_api.rs> |
| `src/prompts.rs` | Prompt template loading and rendering for code generation tasks. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/prompts.rs> |
| `src/runtime.rs` | Runtime module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime.rs> |
| `src/runtime/approval.rs` | Capability-based approval policies for sandboxed operations. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/approval.rs> |
| `src/runtime/backend.rs` | Model backend types and protocol abstractions for LLM integration. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/backend.rs> |
| `src/runtime/command.rs` | Command execution: one-shot, streaming, PTY, and process group management. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/command.rs> |
| `src/runtime/context.rs` | Async turn execution context, edit-turn driver, and conversation update forwarding. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/context.rs> |
| `src/runtime/context_assembler/mod.rs` | Context assembly orchestration for model turns (file snapshots and prompt construction). Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/context_assembler/mod.rs> |
| `src/runtime/context_assembler/reads.rs` | Candidate-path extraction, snapshot conversion, and related-path inference for context assembly. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/context_assembler/reads.rs> |
| `src/runtime/frontend.rs` | Frontend adapter contracts and runtime-facing input event types. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/frontend.rs> |
| `src/runtime/json_handoff.rs` | ADR-025 canonical runtime JSON handoff types: `RuntimeEnvelope`, `RuntimeEvent`, `RuntimeRequest`, and related contracts. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/json_handoff.rs> |
| `src/runtime/loop.rs` | Runtime event loop orchestration between mode, frontend, and context. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/loop.rs> |
| `src/runtime/edit_loop.rs` | Task-completion edit loop: assemble→model→apply→validate→retry cycle. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/edit_loop.rs> |
| `src/runtime/mode.rs` | Runtime mode trait defining input/update hooks. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/mode.rs> |
| `src/runtime/policy.rs` | Output sanitization and tool-evidence policy helpers. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/policy.rs> |
| `src/runtime/project_instructions.rs` | Project-level instructions loading from `.vex/AGENTS.md` or `AGENTS.md` with token budget enforcement. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/project_instructions.rs> |
| `src/runtime/sandbox.rs` | Command sandboxing trait and implementations with wrapper and probe methods. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/sandbox.rs> |
| `src/runtime/task_state/mod.rs` | Task execution state types and in-memory methods (status tracking, evidence collection). Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/task_state/mod.rs> |
| `src/runtime/task_state/persist.rs` | Task state persistence: save, load, directory discovery, file listing, and active summary reads. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/task_state/persist.rs> |
| `src/runtime/text_util.rs` | UTF-8 aware text truncation utilities respecting byte boundaries. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/text_util.rs> |
| `src/runtime/update.rs` | `UiUpdate` message types emitted from runtime to frontend. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/update.rs> |
| `src/runtime/validation.rs` | Concurrent validation suite: command execution, retry formatting. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/runtime/validation.rs> |
| `src/state.rs` | State module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state.rs> |
| `src/state/conversation.rs` | Conversation module entrypoint and re-exports for split conversation submodules. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation.rs> |
| `src/state/conversation/core.rs` | Main conversation turn loop, streaming event processing, and model/tool round orchestration. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/core.rs> |
| `src/state/conversation/history.rs` | Message history pruning, truncation, and read-file result summarization helpers. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/history.rs> |
| `src/state/conversation/state.rs` | Conversation state types and `ConversationManager` constructors/accessors. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/state.rs> |
| `src/state/conversation/streaming.rs` | Stream block lifecycle helpers, block promotion, and delta emission utilities. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/streaming.rs> |
| `src/state/conversation/tests.rs` | Conversation module tests covering protocol flow, loop guards, and regression anchors. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/tests.rs> |
| `src/state/conversation/tools/mod.rs` | Tool execution dispatch module root; approval gating, guard helpers, and search/config helpers now live under `src/state/conversation/tools/`. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/conversation/tools/mod.rs> |
| `src/state/stream_block.rs` | Structured stream block models and tool status enum. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/state/stream_block.rs> |
| `src/tui_handle.rs` | CLI raw-mode lifecycle and panic-safe restore guard. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/tui_handle.rs> |
| `src/test_support.rs` | Shared test synchronization helpers (e.g., env lock). Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/test_support.rs> |
| `src/tool_preview.rs` | Tool approval preview rendering and read-file snapshot summaries. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/tool_preview.rs> |
| `src/tools.rs` | Tools module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/tools.rs> |
| `src/skills.rs` | Skills registry load/list/install/remove helpers for `.agents/skills`. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/skills.rs> |
| `src/session_notes.rs` | Session notes loading and resolution with memory token budget management. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/session_notes.rs> |
| `src/tools/operator/mod.rs` | Sandboxed file/git tool operator entrypoint with path safety and literal search submodules. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/tools/operator/mod.rs> |
| `src/turn_evidence.rs` | Turn evidence recording: input, response, file changes, and command history. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/turn_evidence.rs> |
| `src/types.rs` | Types module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/types.rs> |
| `src/types/api_types.rs` | API request/response content and streaming event structs/enums. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/types/api_types.rs> |
| `src/types/model_profile.rs` | ModelProfile configuration for LLM models with system prompt and parameters. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/types/model_profile.rs> |
| `src/ui.rs` | UI module entry and re-exports. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui.rs> |
| `src/ui/editor/mod.rs` | Text input editor module root with history, undo/redo stacks, and cursor management; editor tests now live under `src/ui/editor/tests.rs`. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/editor/mod.rs> |
| `src/ui/input_metrics.rs` | Input editor row/width metrics for viewport-safe rendering. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/input_metrics.rs> |
| `src/ui/layout.rs` | Ratatui pane layout splitting and geometry helpers. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/layout.rs> |
| `src/ui/render/mod.rs` | Ratatui render module root for status, history, input, and overlays; transcript rendering and tests now live under `src/ui/render/`. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/ui/render/mod.rs> |
| `src/util.rs` | Shared utility functions (bool/env parsing and endpoint helpers). Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/util.rs> |
| `src/usage.rs` | Token usage tracking structures for turns and sessions. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/usage.rs> |
| `src/workspace.rs` | Repo-root and workspace-relative path helpers for repo-scoped state. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/src/workspace.rs> |
| `tests/integration_test.rs` | Integration tests for config validation behavior. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/tests/integration_test.rs> |
| `tests/layout_underflow_tests.rs` | TUI layout constraint and underflow regression tests. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/tests/layout_underflow_tests.rs> |
| `tests/signal_handling_tests.rs` | Command session cancellation and process group signal tests. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/tests/signal_handling_tests.rs> |
| `tests/stream_parser_tests.rs` | Stream parser protocol and fragmentation tests. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/tests/stream_parser_tests.rs> |
| `tests/tool_operator_tests.rs` | Tool operator behavior/security tests for file and git actions. Raw: <https://raw.githubusercontent.com/aistar-au/vexcoder/main/tests/tool_operator_tests.rs> |

---

## Reference

- [AGENTS.md](AGENTS.md) — bootstrap dependency map for the private operator skill tree
- [ADR index](adr/ADR-README.md) — architectural decisions and their rationale
