# ADR-045: Replay-First Task Document and Single-Writer State

- **Status:** Proposed
- **Date:** 2026-04-07
- **Deciders:** Core maintainer
- **Depends on:** ADR-025, ADR-029, ADR-030, ADR-035, ADR-041, ADR-043
- **Deprecates:** None
- **Deprecated by:** None

## Context

The repository already has the right foundation: a shared `TaskDocument`, a
typed `RuntimeEvent` vocabulary, a `TaskDocumentCondenser` over those events,
and a `ConversationManager` that wraps condenser turn lifecycle. That foundation
is better structured than many comparable tools manage to build.

The unresolved problem is that the condenser is not given sole authority over
the document. Several layers bypass it:

- `streaming.rs` directly assigns to assistant block content and `ttft_ms`
  inside `TaskDocument`, instead of routing all content deltas as
  `RuntimeEvent`s through the condenser.
- `model_update.rs` directly writes task status, prompt progress, timings,
  changed files, command-session maps, and approval state, all without
  condenser involvement.
- `turn.rs` directly mutates the command-session map instead of routing
  `CommandSession` `TurnEntry` events through the condenser.

Simultaneously, the normalizer in `json_handoff.rs` explicitly drops
`CommandSessionStarted`, `CommandSessionAttached`, `CommandSessionFinished`,
`EditLoopComplete`, `ContextCompacted`, and `ServerMetadata`. Those events
correspond to fields that exist in the `TaskDocument` model, so the document
model and the replayable event stream are structurally mismatched: the model
claims to track state that no event ever produces.

Finally, `task_state_bridge.rs` labels its persistence path
`persistable_snapshot` and notes it is a "lossy projection". The restore path
zeros timings, clears model metadata, and always exits with `active_turn: None`.
Resume therefore rebuilds a degraded evidence view, not the full live state.

Without resolving these three gaps — sole-writer authority for the condenser,
complete event coverage for every document field, and full-fidelity persistence
— the following capabilities remain impossible to build correctly:

- deterministic replay from persisted history
- exact resume of interrupted or handed-off sessions
- authoritative debug, export, and audit surfaces
- first-class session and turn rollback distinct from file-level undo
- parity proof tests between live execution and restored execution

This ADR defines the end-state architecture that resolves all three gaps.

## Decision

The runtime SHALL adopt a **replay-first, single-writer** architecture. The
`TaskDocument` is always a deterministic projection of an append-only event
log. Exactly one module — `TaskDocumentCondenser` — writes to it. Every other
module reads from it.

## Definitions

The following terms define the four components of the architecture. Each
component has a single bounded responsibility. The boundaries do not overlap.

---

### RuntimeEventLog

**Responsibility: authoritative persisted event sequence.**

The `RuntimeEventLog` is an append-only ordered sequence of
`RuntimeEnvelope`s. It is the sole authoritative persisted record for a task's
execution history.

`RuntimeEventLog` is authoritative for:

- resume (reconstruct what would be in memory if the runtime had not stopped)
- replay (deterministic reconstruction of `TaskDocument` from scratch)
- session and turn rollback (via rollback markers appended to the log)
- export and reporting surfaces
- debug and audit surfaces

`RuntimeEventLog` does NOT:

- mutate `TaskDocument`
- make orchestration decisions
- derive projections
- expose a query API beyond sequential or checkpoint-relative traversal

Events in the log are immutable once appended. History changes are always
represented by appending a new event (a rollback marker, a branch marker).
Events are never rewritten, truncated, or silently deleted.

---

### TaskDocumentCondenser

**Responsibility: sole writer to `TaskDocument`.**

`TaskDocumentCondenser` (formerly `TaskDocumentReducer`) is the only module
permitted to mutate `TaskDocument` fields that affect runtime semantics, replay
correctness, transcript content, or orchestration decisions.

`TaskDocumentCondenser` owns the following entry points and no others:

- `begin_turn(info: TurnInfo) -> TurnHandle` — initialise a new active turn
  in the document
- `finish_turn(handle: TurnHandle, outcome: TurnOutcome)` — close the active
  turn and commit it to the completed-turn list
- `apply_event(event: RuntimeEvent) -> CondensationSummary` — apply one
  runtime event to the document; return a rendering hint for downstream
  projections
- `restore_from_checkpoint(checkpoint: TaskDocumentCheckpoint)` — restore all
  document fields to full-fidelity checkpoint state, including the active turn

`TaskDocumentCondenser` does NOT:

- append to `RuntimeEventLog`
- read from `RuntimeEventLog`
- decide what event to apply next (that is `ConversationManager`'s decision)
- produce projections beyond `CondensationSummary`
- own persistence logic

No module outside `TaskDocumentCondenser` may write to replay-relevant
`TaskDocument` fields. Replay-relevant fields are all fields that would affect
the outcome of a replay, a resume, a transcript projection, or an orchestration
decision. When in doubt, the field is replay-relevant.

---

### TaskDocument

**Responsibility: accepted in-memory runtime state.**

`TaskDocument` is the exclusive in-memory source of truth for:

- active-turn state (assistant blocks, tool lifecycle, approval state,
  timings, prompt progress, command-session attachment)
- completed-turn history (transcript entries, tool results, validation records)
- session-level metadata (task status, task identity, model metadata)
- context-compaction records
- error state and max-turn termination records

`TaskDocument` is always a projection of the `RuntimeEventLog`. Its content
at any point in time is identical to what would result from replaying all
events in the log up to the same point, optionally starting from a checkpoint.

`TaskDocument` does NOT:

- accept direct field writes from outside `TaskDocumentCondenser`
- persist itself (that is `RuntimeEventLog`'s and the checkpoint writer's job)
- make orchestration decisions (that is `ConversationManager`'s job)
- produce display rows directly (projections do that)

---

### ConversationManager

**Responsibility: coordination owner.**

`ConversationManager` is the only module that holds both `RuntimeEventLog` and
`TaskDocumentCondenser` simultaneously. It drives the event lifecycle from
arrival to document update.

`ConversationManager` owns the sequence:

```text
event arrives from normalizer
→ ConversationManager appends RuntimeEnvelope to RuntimeEventLog
→ ConversationManager calls TaskDocumentCondenser::apply_event
→ TaskDocument is updated
→ ConversationManager evaluates orchestrator decision from TaskDocument
```

`ConversationManager` does NOT:

- mutate `TaskDocument` fields directly outside the condenser call path
- make projection decisions
- own the display row shape

---

### CondensationSummary

**Responsibility: rendering hint only.**

`CondensationSummary` is the return value of
`TaskDocumentCondenser::apply_event`. It signals to projection consumers which
regions of the document changed, to allow incremental rendering without a full
re-projection.

`CondensationSummary` does NOT:

- hold semantic state that is not also present in `TaskDocument`
- serve as the source of truth for any orchestration or persistence decision
- replace a projection: it is a hint, not a view

---

### Projections

**Responsibility: derived views for surfaces.**

Projections are all derived views built from `TaskDocument` or `RuntimeEventLog`:
transcript rows, timeline rows, batch export payloads, evidence summaries, API
response shapes, debug log entries.

Projections do NOT:

- write to `TaskDocument`
- write to `RuntimeEventLog`
- make orchestration decisions
- have their own persistent state

If a projection needs information not available in the accepted state, the
solution is to add a `RuntimeEvent` and a condenser handler, not to store
state in the projection.

---

### Checkpoint

**Responsibility: full-fidelity resume cache.**

A `TaskDocumentCheckpoint` is a persisted point-in-time snapshot of the full
`TaskDocument` state, taken at a well-defined event log position. A checkpoint
accelerates resume and replay by allowing reconstruction to start from a known
good state rather than from the beginning of the event log.

A `TaskDocumentCheckpoint`:

- MUST capture all replay-relevant `TaskDocument` fields, including the
  complete active-turn state, timings, model metadata, tool policy, and
  command-session attachment
- MUST record the `RuntimeEventLog` position at which it was taken
- MUST be sufficient to reconstruct `TaskDocument` by passing it to
  `TaskDocumentCondenser::restore_from_checkpoint` followed by replaying all
  log entries after the checkpoint position

A checkpoint is NOT a `TaskDocument` serialization that drops active-turn
content, zeroes timings, or clears model metadata. That pattern is a
compatibility export (see below).

---

### Compatibility Export

**Responsibility: backward-compatible reporting artifact.**

A compatibility export is a derived artifact in legacy format, produced for
reporting surfaces or older code that cannot yet consume a full checkpoint.

A compatibility export:

- MAY be lossy
- MUST NOT be used as the accepted resume source
- MUST be clearly identified as non-authoritative in its producing code with
  an explicit comment

If a compatibility export is expected at any point to serve as the accepted
resume source, it is no longer a compatibility export. It must be upgraded to
satisfy all `TaskDocumentCheckpoint` requirements.

---

### Rollback Marker

**Responsibility: session-level revert record.**

A rollback marker is a `RuntimeEvent` variant appended to `RuntimeEventLog`
that records the decision to change the active history to an earlier turn or
session boundary.

A rollback marker:

- MUST record the target history boundary (turn index, event log position)
- MUST record the workspace checkpoint reference when workspace state restore
  is needed
- preserves the full audit history — it does not delete prior events

A rollback marker is NOT a file undo checkpoint (that is `UndoCheckpoint` from
ADR-035). File undo and session rollback are distinct concepts that MUST NOT be
conflated.

---

## Invariants

The following invariants extend and sharpen `ADR-030`.

### Invariant A — Single writer

`TaskDocumentCondenser::apply_event`, `::begin_turn`, `::finish_turn`, and
`::restore_from_checkpoint` are the only calls that write to replay-relevant
`TaskDocument` fields.

No module may directly assign or mutate replay-relevant `TaskDocument` fields
by any other path: not via struct field access, not via interior mutability,
not via a helper function that bypasses the condenser entry points.

### Invariant B — Event log is accepted persisted truth

`RuntimeEventLog` is the authoritative persisted record.

Provider-native wire events MUST NOT be appended to `RuntimeEventLog`.
Provider-native inputs are normalized to `RuntimeEvent`s first; only
normalized events enter the log.

### Invariant C — Full event coverage

Every `TaskDocument` field that affects runtime semantics MUST have a
corresponding `RuntimeEvent` variant that produces it and a
`TaskDocumentCondenser::apply_event` arm that applies it.

This applies without exception to: prompt progress, turn timings, command-session
lifecycle, changed-files tracking, context-compaction records, validation
outcomes, task error state, active-turn tool policy, and all `TurnEntry`
variants.

A `TaskDocument` field that exists in source with no `RuntimeEvent` producer
is replay-dark. Replay-dark fields are disallowed.

### Invariant D — Checkpoints are full-fidelity

A `TaskDocumentCheckpoint` MUST capture all replay-relevant document fields.

A snapshot that exits restore with `active_turn: None`, with zeroed timings, or
with cleared model metadata is not a checkpoint. It is a compatibility export
and MUST be labeled as one. It MUST NOT be used as the accepted resume source.

### Invariant E — Projections derive from accepted state only

All projections build exclusively from `TaskDocument` or `RuntimeEventLog`.

`CondensationSummary` and similar rendering-hint types MAY guide incremental
projection updates, but they MUST NOT be the sole repository of any piece of
semantic state. If state lives only in a hint, it must be moved to
`TaskDocument`.

### Invariant F — Active turn is part of full resume

A full resume MUST restore the active-turn document state to its exact
condition at interruption, including: assistant block content, tool lifecycle
state, approval state, command-session attachment, timings, prompt progress,
and any metadata needed for the next orchestrator decision.

A resume path that reconstructs only completed turns and starts the next turn
fresh from a blank `ActiveTurnDocument` does not satisfy this ADR.

### Invariant G — Schema closure

Every `TurnEntry` variant, every `RuntimeEvent` variant intended to survive
replay, and every replay-relevant `TaskDocument` field MUST have all of the
following simultaneously:

- at least one live producer in source code
- a `TaskDocumentCondenser::apply_event` arm that handles it
- a test that proves the event produces the expected document mutation
- a test that proves a replay of that event produces an identical mutation

A variant or field that is missing any of the above is incomplete schema.
Incomplete schema MUST be completed or removed before the containing type is
accepted.

## Ownership model

This section defines who owns what and where the non-overlap boundaries are.

### Who appends to RuntimeEventLog

`ConversationManager` only. No other module appends to the log directly.

### Who calls TaskDocumentCondenser

`ConversationManager` only, for live event application.
The resume path (owned by `src/app/turn.rs` and `ConversationManager`) may
also call `restore_from_checkpoint` and then replay tail events.
Test harnesses may call the condenser directly to verify mutation behavior.

### Who reads TaskDocument

`ConversationManager`, the orchestrator, and all projection modules. Reading
is unrestricted; writing is restricted to `TaskDocumentCondenser` entry points.

### Who reads RuntimeEventLog

The resume path and the replay path, driven by `ConversationManager`.
Projection modules may read the log for export or audit surfaces.

### Who may NOT write to TaskDocument

- `streaming.rs`
- `model_update.rs`
- `turn.rs` outside of condenser calls
- application layer code in `src/app/`
- test helpers (except via condenser entry points)

### What compatibility exports are allowed to do

A compatibility export such as `persistable_snapshot` in `task_state_bridge.rs`
may produce a derived artifact for backward-compatible surfaces. It MUST be
labeled as non-authoritative in its source. It MUST NOT be called from any
resume or replay path while full-fidelity checkpoints are available.

## Required RuntimeEvent coverage

`RuntimeEvent` MUST include explicit variants covering all of the following.
Each entry also states the `TaskDocument` field it produces.

| Coverage area | Required variants | Document target |
| :--- | :--- | :--- |
| Turn boundaries | `TurnStarted(TurnInfo)`, `TurnFinished(TurnOutcome)` | `active_turn`, `turns` |
| Assistant blocks | `AssistantBlockStarted(BlockId, BlockKind)`, `AssistantTextDelta(BlockId, String)`, `AssistantBlockFinished(BlockId)` | `active_turn.entries` |
| Tool lifecycle | `ToolCallStarted(ToolCallId, ToolName, Input)`, `ToolResultReceived(ToolCallId, ToolResult)` | `active_turn.entries` |
| Approval lifecycle | `ApprovalRequested(ToolCallId, ToolName)`, `ApprovalResolved(ToolCallId, ApprovalOutcome)` | `active_turn.approval_state` |
| Prompt and timing metadata | `PromptProgressUpdated(PromptProgress)`, `TurnTimingRecorded(TurnTimings)` | `active_turn.prompt_progress`, `active_turn.timings` |
| Command-session lifecycle | `CommandSessionStarted(SessionId, SessionMeta)`, `CommandSessionAttached(SessionId)`, `CommandSessionOutputChunk(SessionId, Chunk)`, `CommandSessionFinished(SessionId, ExitCode)`, `CommandSessionCancelled(SessionId)`, `CommandSessionFailed(SessionId, Reason)` | `active_turn.command_sessions` |
| Validation lifecycle | `ValidationStarted(ValidationId)`, `ValidationFinished(ValidationId, ValidationOutcome)` | `active_turn.entries` |
| Context compaction | `ContextCompacted(CompactionRecord)` | `task_doc.context_compaction` |
| Task error and termination | `TaskErrored(ErrorDetail)`, `MaxTurnsReached(TurnCount)` | `task_doc.info.status` |
| Checkpoints and rollback | `CheckpointCreated(CheckpointId, LogPosition)`, `RollbackMarkerAppended(TargetBoundary, WorkspaceCheckpointRef)` | event log structural |

If a conceptual event exists in the document model but has no `RuntimeEvent`
variant, the variant MUST be added before the corresponding field is used in
any production code path.

## What we will not do

The following patterns are explicitly disallowed.

### No dual writers

Code in `streaming.rs`, `model_update.rs`, `turn.rs`, or any module in
`src/app/` MUST NOT write directly to replay-relevant `TaskDocument` fields.

Explicitly disallowed: direct struct field assignment or mutation on
`TaskDocument` or `ActiveTurnDocument` outside `TaskDocumentCondenser` method
bodies.

### No lossy resume authority

`persistable_snapshot` and `restore_from_snapshot` in `task_state_bridge.rs`
MUST NOT be the accepted resume path. They are a compatibility export.

### No replay-dark fields

A `TaskDocument` field that has no `RuntimeEvent` producer MUST NOT remain in
production source code. It must be given a producer or removed.

### No projection-only semantics

`CondensationSummary` and any similar hint type MUST NOT become the only
location where a piece of state exists. If information appears only in a
rendering hint, it belongs in `TaskDocument` instead.

### No provider-native truth

Provider-native wire names, stream lifecycle event names, or stop-reason
strings MUST NOT propagate into `TaskDocument` writes, orchestration decisions,
or projection logic.

### No silent history truncation

Rollback MUST be implemented as a rollback marker appended to
`RuntimeEventLog`. Silently truncating or rewriting the event log is
not allowed.

### No session rollback via file undo alone

`UndoCheckpoint` (ADR-035) is the file-mutation undo mechanism. Session and
turn rollback requires rollback markers in `RuntimeEventLog`. The two mechanisms
are complementary and address different problems. They MUST NOT be conflated.

### No incomplete schema

A `TurnEntry` variant or `RuntimeEvent` variant with no live producer in
source code MUST be removed or completed before the next accepted merge that
touches the enclosing type.

## Consequences

### Positive

- Resume becomes exact: a resumed session is indistinguishable from an
  uninterrupted one, because both are projections of the same event sequence.
- Replay becomes the universal explanation path for all surfaces: transcript,
  export, debug, and any future API view are all derived from the same source.
- Session and turn rollback become first-class defined operations rather than
  ad hoc workarounds.
- Multi-surface projection parity becomes directly testable at the document
  boundary rather than requiring end-to-end integration tests.
- `TaskDocumentCondenser` becomes a real architectural gate: code review can
  reject dual-writer violations mechanically.

### Negative

- The `RuntimeEvent` vocabulary must grow to cover all previously uncovered
  state transitions. Each new variant requires a condenser arm, a projection
  handler, and two test cases (live apply + replay apply).
- Storage requirements increase because event history is no longer summarized
  away. Bounded command output and compaction metadata are both now
  first-class events.
- Direct mutation shortcuts in `streaming.rs` and `model_update.rs` must be
  replaced by event emission. This is refactoring, not net-new behavior.
- Resume tests and parity tests add to compile time and test runner time.

## Non-goals

This ADR does not:

- redefine provider wire-parsing rules already covered by ADR-025 or ADR-029
- define a specific storage format or backend for `RuntimeEventLog`
- define user-facing slash-command syntax for rollback or checkpoint
  operations
- prescribe Rust module layout beyond the single-writer ownership boundary

## Relationship to existing ADRs

### ADR-025

Remains the accepted `RuntimeEnvelope` contract. This ADR additionally
requires that the envelope sequence is the authoritative persisted history,
not only a transport handoff artifact.

### ADR-029

Remains correct on stream parser completeness and additive `TaskState` coverage.
The `persistable_snapshot` / `restore_from_snapshot` path defined in ADR-029
is reclassified here as a compatibility export. It MUST NOT serve as the
accepted resume mechanism once full-fidelity checkpoints are available.

### ADR-030

Remains correct on orchestration ownership and provider-event boundaries. This
ADR tightens ADR-030's ownership model by making sole-writer authority for
`TaskDocumentCondenser`, complete event coverage, and full-fidelity
persistence all explicitly required rather than aspirational.

### ADR-035

Remains the file-level undo decision. This ADR defines a complementary but
distinct concern: replay-aware session and turn rollback via rollback markers.
The two MUST NOT be conflated.

### ADR-034 (Multi-Agent / Parallel Task Execution)

ADR-045 supplies the replay-first, single-writer substrate required by
ADR-034's session-task graph, worktree leases, and orchestrator-owned
lifecycle.

- All session-task metadata (`agent_id`, `worktree_path`, `lifecycle_state`,
  heartbeats) is **replay-relevant** and MUST be covered by `RuntimeEvent`
  variants in accordance with Invariant C. No multi-agent coordination field
  may be written to `TaskDocument` outside `TaskDocumentCondenser`.
- Parallel fan-out and join operations have deterministic replay parity:
  every agent sub-turn is appended to `RuntimeEventLog` in arrival order,
  enabling exact resume of interrupted multi-agent sessions without semantic
  loss.
- Rollback markers defined in ADR-045 provide safe session-level revert
  boundaries that respect the worktree isolation model defined in ADR-034.
  A rollback marker MUST reference the workspace checkpoint for the specific
  worktree being reverted.

*ADR-034 defines the orchestration and agent model. ADR-045 guarantees the
state consistency and auditability that model relies upon.*

## Verification requirements

The repository MUST prove all of the following through named tests.

1. Applying the full `RuntimeEventLog` from position zero reconstructs the
   same `TaskDocument` as the live uninterrupted session at the same point.

2. Applying a `TaskDocumentCheckpoint` followed by replaying the subsequent
   tail of the event log produces the same `TaskDocument` as full replay from
   position zero.

3. Resuming from a checkpoint restores the same active-turn semantics as the
   live session: same assistant block content, same tool lifecycle state, same
   approval state, same timings, same command-session attachment.

4. Transcript and timeline projections derived after replay match those derived
   from the live session for the same event sequence.

5. Rollback markers restore the correct active history boundary and reference
   the correct workspace checkpoint.

6. Every replay-relevant `TaskDocument` field has at least one test that
   proves the field is set via a `RuntimeEvent` + condenser path, not via
   direct assignment.

7. Every `TurnEntry` variant has producer, condenser, projection, and replay
   test coverage.

8. No module outside `TaskDocumentCondenser` directly assigns to
   replay-relevant `TaskDocument` fields. A compile-time enforcement path
   (restricted visibility or a lint) is preferred over runtime detection.

9. Provider-native event names do not appear in projection logic or
   orchestration decision code.

10. The compatibility export (`task_state_bridge.rs` paths) is never called
    from any resume or replay path while a full-fidelity checkpoint is
    available.
