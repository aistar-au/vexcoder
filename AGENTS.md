# Agent Bootstrap

> **Skills are NOT in this repo.**
> All `SKILL.md` files live in `../vexdraft/.agents/skills/`.
> There is no `.agents/` directory in this repo; do not probe or create one.
> This file is step zero. Loading skills from `vexdraft` is step one.
> Bootstrap table is below; follow it before any other action.

`vexcoder` is the public product repo. The dispatcher skills, PR-contract
rules, commit-debug tooling, docs automation, and roadmap automation that drive
agent workflows now live in the internal private repo `../vexdraft`.

This file is the bootstrap dependency map for agents working in `vexcoder`.
Read it first, then follow the linked local and internal-repo sources.

## Required internal layout

The expected checkout layout is:

```text
~/git-repo/
├── vexcoder/
└── vexdraft/
```

If `../vexdraft` is missing, skill bootstrap is incomplete and dispatcher-owned
workflows cannot be verified from this repo alone.

## Session start sync (required)

Before any work in this repo — reading, drafting, implementing, or verifying —
run the following in both working trees:

```sh
# In ~/git-repo/vexcoder
git fetch origin --prune
git merge --ff-only origin/main
test "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" \
  && echo "vexcoder in sync" || { echo "vexcoder MISMATCH — do not proceed"; exit 1; }

# In ~/git-repo/vexdraft
git fetch origin --prune
git merge --ff-only origin/main
test "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" \
  && echo "vexdraft in sync" || { echo "vexdraft MISMATCH — do not proceed"; exit 1; }
```

A stale local HEAD in either repo is a hard stop. Confirm sync before the first
tool call in any session. See `vex-remote-contract` Hard Rule 34.

## Bootstrap dependencies

Read these files in order before producing dispatch prompts, PR motivation, or
review text:

| Order | File | Why it is a dependency |
| :--- | :--- | :--- |
| 1 | `../vexdraft/.agents/skills/vex-local-bash/SKILL.md` | Local drafting rules for summaries, findings, and PR motivation text. |
| 2 | `../vexdraft/.agents/skills/vex-remote-contract/SKILL.md` | Cross-repo branch verification, raw URL validation, PR-body posting, and push/merge contract. |
| 3 | `../vexdraft/.agents/skills/vex-rust-arch/SKILL.md` | Rust-specific architecture guidance when the task touches `src/**/*.rs`, `tests/**/*.rs`, or ADR-024 gaps. |

Supplemental dependency files are loaded only when the task scope requires
them:

| Trigger | File | Purpose |
| :--- | :--- | :--- |
| ADR-024 parity or gap planning | `../vexdraft/.agents/skills/vex-remote-contract/references/adr-024-gap-map.md` | Gap inventory and dependency notes for ADR-024 work. |
| Rust coding task needs expanded language rules | `../vexdraft/.agents/skills/vex-remote-contract/references/rust-rules.md` | Rust implementation constraints used by the dispatcher workflow. |

## Local repo sources

After the bootstrap dependencies above, read the repo-local sources that define
the product-side constraints:

| File | Role |
| :--- | :--- |
| `CONTRIBUTING.md` | Contributor and workflow reference for this repo. |
| `adr/ADR-README.md` | Index of all open and completed ADRs. |
| `adr/ADR-021-codebase-audit-dead-weight-duplication-shared-code-opportunities.md` | Audit cleanup and follow-up maintenance context. |
| `adr/ADR-022-amendment-2026-03-13.md` | Command-execution amendment that aligns ADR-022 with the current command-session runtime. |
| `adr/ADR-022-free-open-coding-agent-roadmap.md` | Free/open roadmap target and config/interface decisions. |
| `adr/ADR-023-deterministic-edit-loop.md` | Locked edit-loop behavior and EL batch sequencing. |
| `adr/ADR-024-zero-licensing-cost-agent-parity-gaps.md` | Parity-gap inventory, command surface, and deferred work. |
| `adr/ADR-025-runtime-json-handoff-contract.md` | Phase I canonical runtime JSON handoff contract and post-gate dispatch entry point. |
| `adr/ADR-026-localapiserver-transport-binding.md` | Phase I transport binding, TLS rules, and post-ADR-025 dispatch sequence. |
| `adr/ADR-027-full-screen-tui-command-session-capture.md` | Full-screen TUI and command-session capture alignment, plus current implementation limits. |
| `Makefile` | Local verification entry points and architecture gate wrappers. |

## Current cross-repo dependency state

For dispatcher-owned workflow and skill routing, the current active ADR set is
ADR-021 through ADR-027. `adr/ADR-README.md` remains the full source of
truth for the broader open-ADR list.

| ADR | Current state | Dependency note |
| :--- | :--- | :--- |
| ADR-021 | Accepted, follow-up maintenance remains | Audit and cleanup items can still affect `src/`, tests, or docs shape. |
| ADR-022 | Proposed, with 2026-03-13 amendment | Sets the free/open roadmap target that the private dispatcher skills are validating against. |
| ADR-023 | Locked | EL-07 is merged and ADR-024 Phase A is reconciled complete. The current next edit-loop batch is EL-08 (`ModelProfile` config integration via layered config), followed by EL-09. Target files for EL-08 are `src/config.rs`, `src/app.rs`, `src/api/client.rs`, `src/bin/vex.rs`, `docs/src/configuration.md`, and the config-layer tests in `src/config.rs` / `tests/integration_test.rs`. |
| ADR-024 | Proposed | Defines gap work around layered config, MCP, skills, export, and related parity surface. |
| ADR-025 | Proposed | First post-gate Phase I dispatch target (`PI-09` through `PI-12`); implementation remains gated on milestone-1 correctness validation. |
| ADR-026 | Proposed | Follows ADR-025 closeout and ADR-024 reconciliation (`PI-13` through `PI-16`); implementation remains gated on milestone-1 correctness validation. |
| ADR-027 | Accepted | Full-screen TUI with command-session capture, concurrent validation, and current follow-up limits. Supersedes ADR-018 and ADR-019. |

## Verification baseline

Minimum local verification for repo changes:

- `cargo test --all-targets`
- `make gate-fast`
- `bash scripts/check_no_alternate_routing.sh`
- `bash scripts/check_forbidden_imports.sh`

If the changed paths include `src/**/*.rs` or `tests/**/*.rs`, the dispatcher
workflow in `../vexdraft` also expects the internal-repo review path described in
`../vexdraft/.agents/skills/vex-remote-contract/SKILL.md`.

## Dispatcher contract notes

These points are dependencies because the private skill tree and local ADRs both
rely on them:

- Read-only, planning-only, and audit-only requests stay no-touch until the
  user explicitly asks for implementation.
- File edits are exact unified diffs; do not reconstruct or overwrite whole
  files to apply a hunk.
- Remote writes require explicit user approval before push, commit, PR create,
  or PR update.
- Merge commits to `main` use `git merge --no-ff`; no squash or rebase merge.
- `/clear` clears conversation history while keeping task identity; ADR-024
  also requires it to clear `active_edit_loop`, and the session token
  accumulator resets on `/new` and `/clear`.
- `RuntimeContext` client accessors use `Arc::clone(&self.client)`. Turn
  cancellation remains per-turn via `child_token()` rather than reusing the
  root cancellation token.
- For remote branch and commit operations in the dispatcher workflow, total
  changed payload under 50 KB uses a single MCP `push_files` call; total
  payload at or above 50 KB falls back to local `git push`.

## Dependency summary

`vexcoder` owns public product code, tests, release CI, and local architecture
gates. `vexdraft` owns the private operator skill tree and dispatcher tooling
that batch-review, verify, and post work against this repo. Both sides are part
of the current review contract.
