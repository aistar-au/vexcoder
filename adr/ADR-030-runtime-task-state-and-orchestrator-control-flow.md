# ADR-030: Runtime Task State and Orchestrator Control Flow

- **Status:** Accepted — invariant fixes landed 2026-03-17; verification suite completed 2026-03-25
- **Date:** 2026-03-16
- **Deciders:** Maintainers
- **Depends on:** ADR-023, ADR-025, ADR-027, ADR-028, ADR-029
- **Supersedes:** None
- **Superseded by:** None

## Context

The runtime is no longer a thin chat client. It already contains or is actively
gaining:

- a deterministic edit loop
- managed tool execution and command sessions
- provider stream parsing across multiple protocol shapes
- canonical runtime handoff types and schemas
- batch/export/evidence consumers that should not depend on provider-native
  wire values

Those pieces exist across multiple ADRs, but the repository does not yet define
a single normative execution model that answers all of the following clearly:

1. What is the difference between a provider event, a runtime event, and task
   state?
2. Which layer owns truth about what is currently happening?
3. Which layer decides whether the task continues or stops?
4. How do managed command sessions fit into the task loop?
5. Which downstream surfaces are allowed to observe provider-native event
   names?
6. What is the canonical control flow from streamed provider output to task
   completion?

ADR-031, ADR-032, ADR-033, batch/export derivations, and task handoff/resume
surfaces all consume this control-flow contract. When one worker or surface
resumes a task created by another, the handoff depends on the same runtime
ownership rules: provider-native events remain non-authoritative, managed
command sessions remain runtime-owned beyond stream chunks, and tool or command
results re-enter task state before the next turn.

Without a dedicated definition, the architecture can drift toward incorrect
patterns such as:

- treating provider `message_stop` or equivalent wire events as task completion
- letting provider-native event names leak into runtime, batch, export, or UI
  logic
- coupling subprocess lifetime to provider stream lifetime
- letting UI state become the source of truth for task execution
- treating tool execution as a side effect instead of part of the orchestrated
  runtime loop

## Decision

The runtime SHALL be defined as a **task-state-owned orchestrator**.

The canonical runtime execution model is:

```text
provider event arrives
→ normalize to runtime event
→ update task state
→ orchestrator checks state
→ execute next required action
→ continue until runtime completion
```

This flow is normative.

## Definitions

### Provider event

A provider event is a transport- or protocol-specific unit received from an
inference backend or local server.

Examples include:

- stream lifecycle events
- content block start/delta/stop events
- message stop events
- usage chunks
- heartbeat or ping events
- provider-native error events

Provider events describe what arrived over the wire. They do not define runtime
truth and do not directly control task completion.

### Runtime event

A runtime event is a canonical, runtime-owned event emitted after
provider-native input has been normalized into the repository's internal
execution model.

Runtime events exist so that downstream consumers can rely on stable semantics
independent of provider protocol shape.

### Task state

Task state is the durable runtime-owned record of what is currently true for a
task and its turns.

Task state is the source of truth for:

- current turn and sequence position
- accumulated conversation/tool/validation history
- pending approvals
- active managed command sessions
- mutation and validation status
- blocked, interrupted, failed, or completed state
- runtime-owned completion decisions

### Orchestrator

The orchestrator is the runtime control authority that decides what happens next
after task state is updated.

The orchestrator is responsible for continuation, pausing, retrying, validation
sequencing, and terminal completion.

## Ownership model

### Provider adapter / parser layer

This layer owns:

- transport calls
- provider stream parsing
- provider-native usage extraction
- provider-native error extraction
- compatibility handling across protocol variants

This layer does not own:

- task completion
- task truth
- command lifecycle
- agent continuation policy

### Normalization layer

This layer owns:

- mapping provider-native events into canonical runtime events
- runtime-owned sequencing and envelope emission
- removal of provider-native event leakage from downstream consumers

This layer does not own:

- stop/continue decisions
- tool execution policy
- subprocess lifetime policy

### Task state

Task state owns durable truth about the task.

All runtime decisions MUST be made against task state rather than raw provider
events.

### Orchestrator / deterministic edit loop

This layer owns:

- whether the task continues
- whether a tool or command must execute
- whether validation must run
- whether approval must pause execution
- whether the task is complete
- whether a no-op or non-mutating turn requires retry guidance

### Managed command session

Managed command session state owns:

- subprocess attachment
- subprocess output streaming
- subprocess interruption/cancellation
- subprocess completion
- bounded result shaping back into model-visible context
- full transcript visibility for UI or evidence consumers where applicable

Command session lifetime MUST be owned by the runtime, not by the provider
stream lifecycle.

### UI / batch / export / evidence

These surfaces MUST consume canonical runtime events and task-derived state
only.

They MUST NOT depend on provider-native event names or provider-specific stream
semantics.

## Canonical control flow

The runtime SHALL operate according to the following logical sequence:

1. Create or resume task state.
2. Build current turn context from task state.
3. Call the provider through a transport adapter.
4. Parse incoming provider-native events.
5. Normalize them into canonical runtime events.
6. Apply those runtime events to task state.
7. Ask the orchestrator what action is required next.
8. If the next action is tool execution, execute the tool and write the result
   back into task state.
9. If the next action is a managed command session, start or continue the
   subprocess under runtime ownership, stream output, and write the command
   result back into task state after completion.
10. If the next action is patch application or validation, execute it under
    runtime policy and write the result back into task state.
11. If the next action is approval wait, pause the task under runtime-owned
    pending-approval state.
12. Continue until runtime-owned completion criteria are satisfied.

## Invariants

The following are mandatory invariants.

### Invariant 1: provider events are never task truth

Raw provider events MUST NOT be treated as authoritative task state.

### Invariant 2: provider stream completion is not task completion

A provider-native stream end, message stop, or equivalent transport event MUST
NOT by itself terminate a task.

### Invariant 3: runtime state decides continuation

Continuation and terminal completion MUST be decided by the orchestrator after
task state has been updated.

### Invariant 4: managed command sessions outlive provider stream chunks

A managed subprocess MAY continue running after a provider response has ended.
Its lifecycle remains runtime-owned until interruption or exit.

### Invariant 5: tool and command results re-enter task state

Every executed tool, command, validation, or patch result that influences
future reasoning MUST be recorded back into task state and become eligible
context for the next turn.

### Invariant 6: downstream consumers do not inspect provider-native event names

UI, batch mode, export, evidence, and similar consumers MUST depend on
canonical runtime events or task-derived summaries only.

### Invariant 7: application facade is not the orchestrator

UI/application facade code MAY reflect runtime state and forward user actions,
but it MUST NOT become the source of execution truth or the owner of
continuation policy.

## Required task state surface

The concrete type layout may evolve, but task state MUST be capable of
representing at least:

- task identity
- turn identity
- runtime sequence progression
- accumulated conversation history
- tool calls and tool results
- validation runs and outcomes
- patch/mutation status
- pending approval state
- managed command session attachment and lifecycle
- blocked/interrupted/failed/completed terminal states
- maximum-turn and retry conditions

## Managed command session rules

Managed command sessions are part of the orchestrated task loop, not
side-channel behavior.

When the model requests command execution:

1. The orchestrator decides whether execution is allowed or approval-gated.
2. The runtime starts the subprocess under managed session control.
3. UI-facing output may stream continuously.
4. Model-visible command result shaping MAY be bounded or summarized.
5. The command does not become complete until the runtime observes process exit
   or interruption.
6. The command result is written back into task state.
7. The orchestrator then decides the next step.

A command session MUST NOT be considered complete merely because the provider
has stopped streaming.

## Completion rules

A task is complete only when runtime-owned completion conditions are satisfied.

Examples of valid completion conditions include:

- the orchestrator determines no further action is required
- the requested edits and required validations have succeeded
- an explicit final response is consistent with task state and no further
  runtime action is pending
- the task reaches a runtime-owned terminal failure or max-turn condition

Invalid completion signals include:

- provider-native stream end
- provider-native stop reason alone
- a single tool return without orchestrator evaluation
- command launch without command completion handling
- UI inactivity

## Consequences

### Positive

- keeps provider protocol details out of runtime semantics
- gives batch/export/evidence a stable contract
- makes local-server transport work additive rather than invasive
- clarifies the difference between stream parsing, task truth, and
  orchestration
- protects long-running shell jobs from premature termination due to
  stream-bound reasoning
- makes retry, validation, and approval flows easier to reason about and test

### Multi-agent orchestration dependency

ADR-030 was originally the control-flow foundation for UI batches. It is now
also the semantic correctness guarantee for multi-agent handoffs.

When Agent B resumes a task started by Agent A, the six invariants defined here
are exactly what ensures the handoff is coherent:

- **Invariant 1** (provider events are never task truth) prevents Agent B from
  inheriting stale provider-native artefacts left by Agent A's session.
- **Invariant 4** (command sessions outlive provider stream chunks) ensures
  managed subprocesses survive a handoff boundary and remain runtime-owned.
- **Invariant 5** (tool results re-enter task state) guarantees that Agent B
  sees the full tool-result record accumulated by Agent A, not a partial
  transcript.

Without these invariants proven end-to-end, multi-agent orchestration has
undefined behaviour at handoff points. The two invariant patches from
2026-03-17 are in the tree; the full verification suite is completed as of
2026-03-25.

### Negative

- requires explicit task-state updates instead of informal propagation
- may require refactoring code paths that currently react too directly to
  provider-native events
- adds pressure to keep normalization and task-state transitions well tested

## Non-goals

This ADR does not:

- redefine provider-specific parsing formats
- replace the canonical runtime schema work of ADR-025
- replace the deterministic edit-loop policy details of ADR-023
- replace the managed command-session mechanics of ADR-027
- define the local API server transport binding of ADR-026
- mandate a particular Rust module layout

## Relationship to existing ADRs

### ADR-023 — deterministic edit loop

ADR-023 defines core loop behavior. This ADR clarifies that the edit loop is
the orchestrator and that its decisions are based on task state, not
provider-native events.

### ADR-025 — runtime JSON handoff contract

ADR-025 defines canonical runtime events and envelopes. This ADR defines where
those events sit in the control flow and forbids provider-event leakage
downstream.

### ADR-027 — managed command sessions

ADR-027 defines subprocess and command-session behavior. This ADR places those
sessions inside the runtime-owned task loop and clarifies that their lifetime is
independent of provider stream completion.

### ADR-028 — application facade and transport boundaries

ADR-028 keeps application/UI surfaces separated from runtime orchestration. This
ADR reinforces that the app facade is not the source of task truth.

### ADR-029 — stream parser completeness (now accepted)

ADR-029 expands and clarifies provider stream parsing. This ADR states that
parser completeness serves normalization and task-state updates but does not
itself control orchestration. ADR-029 also extends TaskState with plan, session
notes, context compaction, and cache usage — the handoff payload fields that
make multi-agent task resume lossless.

## Implementation guidance

The active implementation sequence remains:

- canonical handoff types and schemas
- normalization layer
- task-state-consistent derivation and testing
- transport binding on top of runtime-owned semantics

In practical terms, the repository should prefer:

```text
provider event
→ normalizer emits runtime event
→ runtime event mutates task state
→ orchestrator decides next action
```

over:

```text
provider event
→ ad hoc consumer reacts directly
→ runtime behavior emerges implicitly
```

## Verification guidance

Coverage should prove at least:

1. provider-native stream end does not automatically complete the task
2. tool results are written back into task state before the next turn
3. managed command sessions remain active until subprocess exit or interruption
4. downstream derivations operate on canonical runtime events rather than
   provider-native names
5. no-op turns and failed validations cause orchestrator-driven continuation
   where policy requires it
6. max-turn and approval pauses are represented in task state and respected by
   the orchestrator

## Verification status

As of 2026-03-25, the repository proves those six coverage points with named
tests in the current tree:

1. provider-native stream end does not automatically complete the task:
   `src/state/conversation/tests/streaming.rs::test_crit_01_protocol_flow`
2. tool results are written back into task state before the next turn:
   `src/state/conversation/tests/streaming.rs::test_crit_01_protocol_flow`
   and
   `src/state/conversation/tests/tool_execution.rs::test_multi_tool_round_collects_results_after_approval_denial`
3. managed command sessions remain active until subprocess exit or interruption:
   `src/state/conversation/tests/tool_execution.rs::test_execute_tool_run_command_streams_managed_session_updates`
   and `src/app/tests/model_turn.rs::test_turn_complete_waits_for_last_command_session_to_finish`
4. downstream derivations operate on canonical runtime events rather than
   provider-native names:
   `src/runtime/json_handoff.rs::test_pi_10_normalization_projects_ui_updates_and_approval_events`
   and
   `src/runtime/json_handoff.rs::test_pi_12_runtime_handoff_round_trips_and_batch_derivation_hold`
5. no-op turns and failed validations cause orchestrator-driven continuation
   where policy requires it:
   `src/runtime/edit_loop.rs::test_edit_loop_skips_validation_when_no_patch_is_applied`
   and
   `src/runtime/edit_loop.rs::test_edit_loop_validation_failure_retries_after_patch_and_stops_at_max_turns`
6. max-turn and approval pauses are represented in task state and respected by
   the orchestrator:
   `src/app/tests/model_turn.rs::test_tool_approval_updates_task_status_until_turn_resumes`,
   `src/app/tests/model_turn.rs::test_tool_approval_request_persists_awaiting_approval_status_in_task_state`,
   `src/app/tests/model_turn.rs::test_tui_edit_loop_completion_persists_max_turn_status_in_task_state`,
   and
   `src/runtime/task_state.rs::test_max_turns_reached_is_distinct_from_completed`

## Invariant violations fixed 2026-03-17

Two implementation gaps found in branch review violated invariants defined in
this ADR.  Both are patched and recorded here for traceability.

### Invariant 1 violation — provider events treated as protocol truth

`should_prefer_chat_compat_wire_protocol()` inspected the URL suffix and
silently overrode the explicit `MessagesV1` protocol config for any URL
ending in `/messages`.  This made a provider-URL heuristic (effectively a
provider-native artefact) control the runtime wire protocol — a violation
of the principle that provider-native values must not determine runtime truth.

The fix limits the heuristic to bare `/v1` base URLs only.  Explicit `/messages`
path suffixes are treated as authoritative MessagesV1 declarations.

### Invariant 6 violation — UI did not observe canonical task state for in-flight steps

`task_activity_rows()` derived its display from `current_turn_tool_invocations`
(completed tool results) but ignored `pending_turn_tool_calls` (in-flight tool
calls recorded in task state).  The activity pane therefore showed a blank or
stale view while a tool was executing — the UI was not continuously reflecting
task state as required by this ADR.

The fix includes in-flight tool calls from `pending_turn_tool_calls` in the
activity row derivation, ensuring the orchestration view remains accurate from
tool-call start through tool-result receipt.  The row set is capped at 6 for
display stability.

> **Implementation note (ADR-031 Batch E):** `task_activity_rows()` and the
> `activity_rows` field were removed in ADR-031 Batch E.  The structured
> timeline renderer now derives step rows directly from canonical task state,
> resolving this invariant violation at the source.

---

## References

- ADR-023 — deterministic edit loop
- ADR-025 — runtime JSON handoff contract
- ADR-027 — full-screen TUI command session capture / managed command sessions
- ADR-028 — application facade and transport boundaries (debug fixes recorded there)
- ADR-029 — stream parser completeness and session persistence
- `../vexdraft/scripts/commit-debug.py` — authoritative cross-repo debug commit tooling
