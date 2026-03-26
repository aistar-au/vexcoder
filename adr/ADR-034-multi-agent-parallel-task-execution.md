# ADR-034: Multi-Agent / Parallel Task Execution — worktree-isolated agent definitions, orchestrator-owned session tasks, watch surfaces, and background lifecycle

- **Status:** Active (Phase A + Phase B-E baseline implemented; see PR #229 and PR #230)
- **Date:** 2026-03-26
- **Deciders:** Core maintainer
- **Depends on:** ADR-024, ADR-025, ADR-026, ADR-030, ADR-033
- **Supersedes:** None
- **Superseded by:** None

## Context

ADR-024 explicitly deferred multi-agent / parallel task execution until after
the first milestone. That deferral covered four distinct concerns that are now
too important to leave implicit:

1. per-agent git worktree isolation for concurrent code-bearing work;
2. pluggable agent definitions and team composition;
3. orchestrator-owned session-task lifecycle, including background execution;
4. operator-visible watch and task-management surfaces.

The current tree already contains the foundations that a multi-agent lane must
consume instead of replacing:

- ADR-025 defines canonical runtime request and event envelopes;
- ADR-026 defines the local transport surface for those envelopes;
- ADR-030 defines task-state-owned orchestration invariants;
- ADR-033 reduces context pressure enough that narrower sub-task prompts and
  agent handoffs are practical in large repositories.

At the same time, the repository still lacks a normative answer for the
questions below:

1. Where are agent roles and teams configured?
2. How are concurrent agents isolated from each other at the filesystem layer?
3. Which layer is allowed to spawn, suspend, resume, or terminate session tasks?
4. How do operators inspect live session-task progress without making the UI the
   source of execution truth?
5. What information must survive export, resume, and cross-surface handoff?

Without a dedicated ADR, the architecture can drift toward unsafe patterns such
as multiple agents mutating one worktree, provider output implicitly launching
session tasks, background jobs outliving task-state tracking, or ad-hoc task
management commands inventing contracts outside ADR-025 and ADR-030.

## Decision

Multi-agent / parallel task execution SHALL be introduced as a dedicated,
post-milestone orchestration lane that extends the existing runtime rather than
creating a second execution model.

### 1. Orchestrator ownership remains absolute

The runtime orchestrator remains the only authority allowed to:

- decompose a parent task into session tasks;
- assign a session task to an agent definition;
- mark a session task as pending, running, blocked, failed, cancelled, or
  completed;
- determine whether a background session task is still live;
- merge session-task results back into parent task state.

Provider-native stream events, UI state, and transport-specific sessions MUST
NOT become the source of truth for session-task lifecycle.

### 2. Agent definitions are explicit and repo-readable

Project-scoped multi-agent definitions live in `.vex/agents.toml`.

The file defines:

- named agent profiles;
- optional teams composed from those profiles;
- per-agent model/profile selection;
- allowed tool-capability envelopes;
- worktree isolation policy;
- concurrency limits.

Illustrative shape:

```toml
[[agents]]
name = "rust-fixer"
profile = "default"
isolation = "worktree"
max_parallel_tasks = 1
allowed_capabilities = ["read-file", "apply-patch", "run-command"]

[[teams]]
name = "parallel-review"
members = ["rust-fixer", "docs-reviewer"]
scheduler = "fan_out_join"
```

No implicit agent inventory may be derived from random prompt text, provider
names, or UI selections alone. If an agent is not declared in the resolved
agent-definition surface, the orchestrator cannot schedule it.

### 3. Worktree isolation is mandatory for concurrent code-bearing agents

Any agent allowed to mutate repository state while another agent is active MUST
run in a dedicated git worktree leased by the orchestrator.

Normative rules:

- one mutable agent task per leased worktree;
- no two concurrent mutable session tasks may share the same worktree;
- read-only session tasks may reuse the parent worktree only when no mutating
  session task is executing there;
- worktree lease ownership is recorded in task state and survives resume.

Worktree paths are runtime-managed state, not user-authored config. The
recommended storage root is under the state directory so leases remain coupled
to persisted task metadata.

### 4. Background lifecycle is part of task state

Background session tasks are first-class task-state entries, not detached process
side effects.

Task state must record, at minimum:

- `agent_id`
- `parent_task_id`
- `worktree_path`
- `lifecycle_state`
- `started_at` / `updated_at`
- last heartbeat or stream sequence observed
- handoff/export summary

Resuming a parent task must reconstruct its session-task graph from task state
before any UI or transport surface renders live status.

### 5. Operator surfaces are observational, not authoritative

The initial operator command surface for this ADR is:

- `/agents` — list configured agents, teams, and live assignment status;
- `/delegate <agent> <prompt>` — request a session task assignment from the
  orchestrator;
- `/watch [task-id|agent-id]` — follow a session-task transcript or status board;
- `vex tasks list` — list persisted parent/session tasks and lifecycle state;
- `vex tasks watch <task-id>` — stream session-task progress in a non-TUI surface.

These commands observe and request orchestrator actions. They do not directly
rewrite task state or bypass approval/sandbox policy.

### 6. Handoff, export, and transport reuse existing canonical contracts

Child-task handoff and resume payloads MUST build on ADR-025 runtime envelopes
and ADR-030 task-state ownership.

Implications:

- exported task graphs must serialize parent/child relationships explicitly;
- background session-task progress exposed via `LocalApiServer` must be projected
  from canonical runtime/task state rather than provider-native wire values;
- resume must restore session-task metadata before replaying any live status to
  the UI or transport surface.

## Implementation phases

### Phase A — Agent definition surface

Deliver `.vex/agents.toml` parsing, validation, team composition rules, and
repo-local discovery.

### Phase B — Worktree lease manager and task-state extensions

Add orchestrator-managed worktree leasing plus the session-task metadata required
for background lifecycle tracking.

### Phase C — Child-task orchestration

Add the parent/session task graph, scheduler decisions, and runtime-owned child
task lifecycle transitions.

### Phase D — Operator task-management surface

Add `/agents`, `/delegate`, `/watch`, and the CLI task-management commands on
top of the Phase A-C runtime contract.

### Phase E — Export / LocalApi projection

Project session-task status, watch streams, and exported task graphs through the
existing batch/export and `LocalApiServer` surfaces.

Merge order is strict: Phase A and Phase B are prerequisites for any
code-bearing parallel execution lane.

## Implementation notes (Phase A + B-E baseline)

Phase A (PR `#229`) delivered `.vex/agents.toml` parsing, validation, and
team composition rules.

Phase B-E baseline (PR `#230`) delivered:

- `src/runtime/session_task.rs` — persisted session-task model
  (`SessionTask`, `SessionTaskStatus`, UUID-scoped IDs to avoid clock-skew
  collisions, heartbeat and handoff-summary tracking);
- `src/runtime/worktree_lease.rs` — orchestrator-managed lease records backed
  by `git worktree add --detach` with a plain-directory fallback for non-git
  test contexts;
- `src/runtime/task_state.rs` — extended with `session_tasks`, `parent_task_id`,
  `agent_id`, `worktree_path`, and backward-compat `child_tasks` serde alias;
- `/agents`, `/delegate`, `/watch/{id}` HTTP routes and handlers in
  `src/server/handlers.rs`, routed through the **ADR-028 application facade**
  via new entrypoints `facade_list_agents`, `facade_delegate_session_task`, and
  `facade_watch_snapshot` in `src/app/task_facade.rs`;
- `/agents`, `/delegate <agent> <prompt>`, `/watch [id]` slash commands in
  `src/app/commands.rs`;
- `vex tasks list` / `vex tasks watch <id>` sub-commands in `src/bin/vex.rs`;
- session-task state wired into batch-mode JSONL summary, Markdown export, and
  existing `TurnEvidence` records.

The `dependency_direction_tests::server_must_not_import_runtime_directly` test
was added to gate future regressions of the ADR-028 boundary.

## Consequences

- ADR-024's multi-agent deferral becomes a defined post-milestone lane instead
  of an open-ended note.
- The repository gains a normative configuration format for pluggable agents
  and teams.
- Concurrent code-bearing work gets a mandatory isolation boundary.
- Background session tasks become resumable and exportable instead of ephemeral.
- The command surface for task management becomes specified before code lands.

## Non-goals

This ADR does not:

- authorize cloud task delegation or remote execution beyond the existing local
  transport posture;
- permit multiple concurrent mutable agents in one worktree;
- define browser-specific UI behavior or GUI dashboards;
- replace the single-agent runtime path for ordinary interactive use;
- let provider output implicitly create session tasks without orchestrator review.

## Implementation notes (debug-pass fixes — PR #234)

A debug-pass analysis of the Phase B-E baseline identified nine observations
patched in `work/vexcoder-debug-pass-fixes` (commit `0ba8351`):

- **O-1** — Require `parent_task_id`: reject `None` to prevent orphan state
  files (`src/app/task_facade.rs`).
- **O-2** — Replace string-comparison error routing with a typed `DelegateError`
  enum via `thiserror` (`src/app/task_facade.rs`, `Cargo.toml`).
- **O-3** — Add sidecar live-count index for `facade_list_agents` to avoid an
  O(n) scan of all task-state files (`src/app/task_facade.rs`).
- **O-4** — Document borrow safety in `validate_team_members`; no logic change
  (`src/agents.rs`).
- **O-5** — Replace hand-rolled `strip_ansi` with the `strip-ansi-escapes`
  crate (`src/app.rs`, `Cargo.toml`).
- **O-6** — Route internal errors through `tracing::error!` in
  `internal_anyhow` for structured observability (`src/server/handlers.rs`).
- **O-7** — Filter comment lines in the dependency-direction enforcement tests
  to avoid false positives on `//` lines (`tests/dependency_direction_tests.rs`).
- **O-8** — Replace unchecked `as u64` cast with
  `try_into().unwrap_or(u64::MAX)` in `now_millis`
  (`src/runtime/session_task.rs`).
- **O-9** — Cap agent name length to 64 bytes at config-validation time to
  prevent `ENAMETOOLONG` on worktree-path construction (`src/agents.rs`).

## References

- `adr/ADR-024-zero-licensing-cost-agent-parity-gaps.md`
- `adr/ADR-025-runtime-json-handoff-contract.md`
- `adr/ADR-026-localapiserver-transport-binding.md`
- `adr/ADR-030-runtime-task-state-and-orchestrator-control-flow.md`
- `adr/ADR-033-hybrid-retrieval-context-architecture.md`
