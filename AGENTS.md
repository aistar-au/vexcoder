# Agent Bootstrap (overhauled for GitHub and local)

> **Skills are not stored in this repo.**
> They live in the `.agents` tree of the adjacent private repo **`vexdraft`**.  
> On a **local checkout**, agent skills are loaded from `../vexdraft/.agents/skills/`.  
> When running on **GitHub.com** or any remote environment, the dispatcher fetches skills from the `aistar-au/vexdraft` repository (the same `.agents` path) and loads them automatically.  
> If that adjacent checkout is missing but the GitHub repo-files MCP server is already enabled, clone or fetch `aistar-au/vexdraft` through the repo-files API first, then load `.agents/skills/` from the fetched `vexdraft` tree before doing any other work.  
> That bootstrap step still applies when the agent session was spawned from a `vexdraft` branch or worktree; loading skills from `vexdraft/.agents` is still mandatory.  
> There is no `.agents/` directory in this repo; do **not** probe or create one.  
> This file is step zero; loading skills from `vexdraft` (locally or remotely) is step one.

`vexcoder` is the public product repo. The dispatcher skills, PR-contract rules,
commit-debug tooling, docs automation, and roadmap automation that drive agent
workflows now live in the internal private repo `../vexdraft`. In remote
contexts (e.g. GitHub runners), the dispatcher resolves `../vexdraft` against
the `aistar-au/vexdraft` repository to load the same sources.

This file is the bootstrap dependency map for agents working in `vexcoder`. Read
it first, then follow the linked local and internal-repo sources.

## Required internal layout

When working **locally**, the expected checkout layout is:

```text
~/git-repo/
├── vexcoder/
└── vexdraft/
```

If `../vexdraft` is missing, local skill bootstrap is incomplete and
dispatcher-owned workflows cannot be verified from this repo alone.  
When running on GitHub.com or another remote environment, the dispatcher
implicitly fetches the `vexdraft` repository and does not rely on a
neighboring checkout.

If the remote environment starts without that adjacent checkout but already has
the repo-files MCP server enabled, use it to clone or fetch `aistar-au/vexdraft`
before reading any skill file. Do not skip bootstrap just because the current
agent was launched from a `vexdraft` branch context; loading the private skill
tree remains a hard prerequisite.

## Session start sync (required)

Before any work in this repo — reading, drafting, implementing, or verifying —
run the following in both working trees *when working locally*:

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

A stale local HEAD in either repo is a hard stop. Confirm sync before the
first tool call in any session. See `vex-remote-contract` Hard Rule 34.  
On GitHub runners the dispatcher checks out fresh `vexcoder` and
`vexdraft` repos automatically, so manual sync is not required.

## Bootstrap dependencies

Read these files in order before producing dispatch prompts, PR motivation, or
review text:

| Order | File | Why it is a dependency |
| :--- | :--- | :--- |
| 1 | `../vexdraft/.agents/skills/vex-local-bash/SKILL.md` (<https://raw.githubusercontent.com/aistar-au/vexdraft/main/.agents/skills/vex-local-bash/SKILL.md>) | Local drafting rules for summaries, findings, and PR motivation text. |
| 2 | `../vexdraft/.agents/skills/vex-remote-contract/SKILL.md` (<https://raw.githubusercontent.com/aistar-au/vexdraft/main/.agents/skills/vex-remote-contract/SKILL.md>) | Cross-repo branch verification, raw URL validation, PR-body posting, and push/merge contract. |
| 3 | `../vexdraft/.agents/skills/vex-rust-arch/SKILL.md` (<https://raw.githubusercontent.com/aistar-au/vexdraft/main/.agents/skills/vex-rust-arch/SKILL.md>) | Rust-specific architecture guidance when the task touches `src/**/*.rs`, `tests/**/*.rs`, or ADR-024 gaps. |

**Note:** In remote environments these same files are loaded from the
`aistar-au/vexdraft` repository. Paths beginning with `../vexdraft/` refer to the
adjacent local checkout when present; the dispatcher resolves them against
remote sources otherwise.

Supplemental dependency files are loaded only when the task scope requires
them:

| Trigger | File | Purpose |
| :--- | :--- | :--- |
| ADR-024 parity or gap planning | `../vexdraft/.agents/skills/vex-remote-contract/references/adr-024-gap-map.md` (<https://github.com/aistar-au/vexdraft/blob/main/.agents/skills/vex-remote-contract/references/adr-024-gap-map.md>) | Gap inventory and dependency notes for ADR-024 work. |
| Rust coding task needs expanded language rules | `../vexdraft/.agents/skills/vex-remote-contract/references/rust-rules.md` (<https://raw.githubusercontent.com/aistar-au/vexdraft/main/.agents/skills/vex-remote-contract/references/rust-rules.md>) | Rust implementation constraints used by the dispatcher workflow. |

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
| `adr/ADR-028-application-facade-and-transport-boundaries.md` | Application/runtime/transport dependency boundary and `src/app.rs` decomposition plan. |
| `adr/ADR-029-stream-parser-completeness-and-session-persistence.md` | Stream parser completeness, normalized SSE metadata retention, and task-state session persistence extensions. |
| `adr/ADR-030-runtime-task-state-and-orchestrator-control-flow.md` | Normative runtime control-flow ownership: provider events normalize into runtime events, task state owns truth, and the orchestrator owns continuation. |
| `adr/ADR-031-operator-surface-ui-overhaul.md` | Operator-surface timeline overhaul, merge-gated UI batching, and task-state-first selection/inspector rules. |
| `Makefile` | Local verification entry points and architecture gate wrappers. |

## Current cross-repo dependency state

For dispatcher-owned workflow and skill routing, the current active ADR set is
ADR-021 through ADR-031. `adr/ADR-README.md` remains the full source of truth
for the broader open-ADR list.

| ADR | Current state | Dependency note |
| :--- | :--- | :--- |
| ADR-021 | Accepted, follow-up maintenance remains | Audit and cleanup items can still affect `src/`, tests, or docs shape. |
| ADR-022 | Proposed, with 2026-03-13 amendment | Sets the free/open roadmap target that the private dispatcher skills are validating against. |
| ADR-023 | Locked | `EL-08` through `EL-13` are now on `main`. The ADR-023 implementation track is complete; milestone-1 validation has passed and the post-gate ADR-025 Phase I work is now active. |
| ADR-024 | Proposed | Defines gap work around layered config, MCP, skills, export, and related parity surface. |
| ADR-025 | Proposed | Phase I kickoff (`PI-09`, `PI-11`) and continuation (`PI-10`, `PI-12`) are implemented in the current tree; ADR-026 `PI-13` through `PI-16` are implemented, and ADR-028 follow-up work now runs against the active facade boundary. |
| ADR-026 | Proposed | Follows ADR-025 closeout and ADR-024 reconciliation (`PI-13` through `PI-16`); `PI-13` through `PI-16` are implemented in the current tree, and downstream work must now preserve the active ADR-028 facade boundary. |
| ADR-027 | Accepted | Full-screen TUI with command-session capture, concurrent validation, and current follow-up limits. Supersedes ADR-018 and ADR-019. |
| ADR-028 | Active | Phase 1 / Phase 2 facade extraction and 2026-03-17 debug fixes are in the current tree; remaining work continues to shrink `src/app.rs` and harden facade/transport seams. |
| ADR-029 | Proposed | Defines stream parser completeness across `messages-v1` and `chat-compat`, including metadata retention for usage, chunk, choice, and tool-call fields plus task-state plan/note/cache persistence. |
| ADR-030 | Active | Defines the task-state-owned orchestrator model so provider events normalize into runtime events, task state remains the source of truth, and command-session lifetime stays runtime-owned. |
| ADR-031 | Active | Defines the timeline-driven operator surface overhaul, merge-gated UI batch ordering, and task-state-first selection/inspector behavior layered on ADR-030. Batch A/B/C implemented: persistent four-region layout, last-turn data preservation, enriched paragraph rendering, adaptive layout engine (dynamic region sizing replacing fixed ACTIVITY_ROWS/INPUT_ROWS), human-readable header, flowing transcript with Unicode markers, inline approval cards in composer. |

## Verification baseline

Minimum local verification for repo changes:

- `cargo test --all-targets`
- `make gate-fast`
- `bash scripts/check_no_alternate_routing.sh`
- `bash scripts/check_forbidden_imports.sh`

If the changed paths include `src/**/*.rs` or `tests/**/*.rs`, the dispatcher
workflow in `../vexdraft` also expects the internal-repo review path described in
`../vexdraft/.agents/skills/vex-remote-contract/SKILL.md`. In remote contexts
these checks run automatically in the CI pipeline.

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
that batch-review, verify, and post work against this repo. Both sides
are part of the current review contract, and the dispatcher resolves `vexdraft`
either from the adjacent local checkout or from the GitHub repository,
depending on the execution environment.
