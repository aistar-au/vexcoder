# ADR-035: Undo Checkpoints and Binary-Safe Rollback

- **Status:** Accepted
- **Date:** 2026-03-30
- **Deciders:** Core maintainer
- **Depends on:** ADR-024, ADR-030, ADR-031
- **Supersedes:** None
- **Superseded by:** None

## Context

ADR-024 Gap 14 deferred `/undo` and per-change checkpoints until a dedicated
ADR defined the rollback strategy. The operator surface now needs a
session-scoped undo mechanism for file-mutating tool calls so users can revert
an accidental write without leaving the current task.

The rollback path must satisfy two constraints that are easy to get wrong if
left implicit:

1. Undo snapshots must preserve exact file bytes, not only UTF-8 text, because
   the agent can touch binary files.
2. Undo state must remain in-memory and session-scoped; it must not depend on
   git state or persist opaque rollback blobs into task-state files.

## Decision

### Rollback model

1. `/undo` is a slash command handled by the TUI operator surface, not a model
   tool call.
2. Before each supported mutating tool call, the runtime captures a checkpoint
   containing the absolute target path, the originating tool name, and the full
   pre-change file bytes when the target existed.
3. If the file did not exist before the mutation, the checkpoint records
   `None`; undo interprets that as "remove the newly created file".
4. Restoring a checkpoint writes the captured bytes back to disk with
   `std::fs::write`, preserving binary content exactly.
5. Checkpoints are held only in the running session's in-memory undo stack and are
   evicted in oldest-first order once the configured maximum depth is reached.

### Scope boundaries

6. The initial checkpoint implementation covers single-path file mutations that
   resolve to one primary file snapshot before the tool runs.
7. Undo does not use git, patch reversal, or transcript replay.
8. The rollback stack is intentionally session-local and is cleared when the
   session ends.
9. Multi-path coordinated rollback remains follow-up work if future tools require it.

## Consequences

- Undo becomes deterministic for text and binary files because it restores raw
  bytes instead of attempting UTF-8 reads.
- Session state stays simple: no disk persistence, no hidden rollback metadata,
  and no dependency on repository cleanliness.
- The first implementation remains intentionally narrow around single-path file
   mutations; if a future tool requires coordinated multi-path rollback, that must be
  specified explicitly in a later ADR amendment or successor ADR.

## Implementation status

Implemented on `work/vexcoder-undo-checkpoints` as of 2026-03-30.

Key source files:
- `src/state/conversation/state.rs`
- `src/app/commands/mod.rs`
- `src/state/conversation/tests/undo.rs`
- `src/config.rs`
- `src/config/load/mod.rs`

## References

- [ADR-024](https://github.com/aistar-au/vexcoder/blob/main/adr/ADR-024-zero-licensing-cost-agent-parity-gaps.md) — parity gap tracker and formal deferment of Gap 14
- [ADR-030](https://github.com/aistar-au/vexcoder/blob/main/adr/ADR-030-runtime-task-state-and-orchestrator-control-flow.md) — session-local runtime state
- [ADR-031](https://github.com/aistar-au/vexcoder/blob/main/adr/ADR-031-operator-surface-ui-overhaul.md) — slash-command operator surface
