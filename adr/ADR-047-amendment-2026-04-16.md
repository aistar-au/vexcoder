# ADR-047 Amendment: API-First Runtime Event Envelope and Trait Reduction (2026-04-16)

**Date:** 2026-04-16
**Status:** Amended
**Amends:** ADR-047
**Related:** ADR-025, ADR-028, ADR-034, ADR-045, ADR-046

## Context

ADR-047 settled the negotiated streaming transport, `tx_` tool-call IDs, and
client-side protocol discovery. It did not fully settle the next architectural
question: what is the primary internal application surface once Vexcoder pivots
from legacy runtime loop traits to an API-first operator-facing application?

The repository still exposes three gaps that matter for that pivot:

1. `RuntimeEnvelope` in `src/runtime/json_handoff.rs` carries `version`,
   `task_id`, `turn`, `seq`, and `event`, but no timestamp, source, request
   correlation, or parent-event linkage.
2. Tool-call lifecycle is still split between explicit tool events and
   transcript-block events, which forces downstream consumers to infer tool
   state from renderer-oriented deltas.
3. Legacy runtime loop traits such as `RuntimeMode`, `FrontendAdapter`, and
    `ToolCallParser` still exist even though the intended direction is one
    accepted API/event surface with the interactive UI acting as a consumer.

## External Precedent Summary

Research reviewed on 2026-04-16 shows a consistent pattern across modern
API-first agent applications and streaming protocols:

- Responses-style clients converge on a single typed request/event surface
  rather than preserving parallel legacy wire APIs behind internal adapters.
  The relevant precedent reviewed for this amendment uses a unified responses
  event stream and rejects its removed legacy chat wire API.
- Mature streaming APIs emit explicit lifecycle events for creation, partial
  output, completion, metadata, and errors. They do not require transcript
  renderers to reconstruct tool state indirectly.
- Tool argument streaming is explicit and fallible. Partial JSON, truncation,
  and invalid tool input are surfaced as protocol states rather than hidden as
  implementation details.
- Control-plane protocols in the JSON-RPC / MCP family treat request IDs,
  notifications, structured errors, cancellation, and progress as first-class
  protocol concerns.
- Event-envelope standards such as CloudEvents standardise `id`, `source`,
  `type`, and `time`; a monotonically increasing sequence number alone is not a
  sufficient event identity or replay contract.

## Decision

### 1. The Runtime Event Envelope Becomes the Primary Internal API

`RuntimeEnvelope` is the accepted application surface consumed by transports,
the interactive UI, persistence, and peer coordination. No new internal trait may
be introduced where a typed request/event surface can represent the contract
directly.

### 2. Envelope Metadata Must Carry Identity, Time, and Correlation

`RuntimeEnvelope` is extended to include:

- `event_id`
- `emitted_at`
- `source`
- optional `request_id`
- optional `parent_event_id`

`task_id`, `turn`, and `seq` remain, but they are no longer the only
correlation mechanism.

### 3. Tool Lifecycle Becomes an Explicit API Contract

Tool execution is represented with explicit event types rather than transcript
inference. The accepted lifecycle is:

- `tool_call_started`
- `tool_call_arguments_delta`
- `tool_call_output_delta` when streaming tool output matters
- `tool_call_completed`
- `tool_call_failed`

Each tool event carries `tool_call_id` and the fields required for replay and
diagnostics, including tool name, status, timing, and truncation or
invalid-JSON markers when applicable.

### 4. Transcript Block Events Are Renderer-Oriented, Not the Public Tool API

`TranscriptBlockStart`, `TranscriptBlockDelta`, and related block events remain
valid renderer-facing content events, but they are not the primary public
expression of tool lifecycle. Tool state must be understandable without
re-parsing transcript deltas.

### 5. Runtime Requests Also Follow an API Contract

Requests that expect a result carry a client-generated `request_id`. Fire-and-
forget notifications remain distinct and do not expect a reply. Interrupt,
cancel, and resume semantics must be modeled explicitly in the request schema
instead of being implied by UI-only control flow.

### 6. UI/Transport Branching Traits Are Simplification Targets

The interactive UI is a consumer of the runtime API, not a separate runtime flavor.
After the envelope and request schemas are extended, the following traits are
follow-up simplification targets:

- `RuntimeMode`
- `FrontendAdapter`

`ToolCallParser` is also a follow-up simplification target once structured tool
calls are the accepted path and the text-protocol fallback is removed.

### 7. True External Boundary Traits Remain Acceptable

Traits that model genuine execution or security boundaries remain valid. The
current keep-list is:

- `ModelBackend`
- `SandboxDriver`
- `ApprovalPolicy`
- `CommandRunner`, if it continues to serve real execution or test-seam needs

The API-first pivot removes internal transport/UI branching traits, not
security and execution boundaries.

## Immediate Implementation Follow-Through

### Phase A — Envelope Metadata

Extend `src/runtime/json_handoff.rs` and `schemas/runtime_envelope_v1.json`
with event identity, timestamp, source, and correlation metadata.

### Phase B — Explicit Tool Events

Promote tool deltas and outcomes to explicit lifecycle events, and stop using
transcript blocks as the only observable source of partial tool arguments.

### Phase C — Request Correlation

Extend `schemas/runtime_request_v1.json` and request handling with request IDs,
explicit notification semantics, and structured cancel/resume support.

### Phase D — Runtime Loop Simplification

Once event coverage is sufficient, collapse `RuntimeMode` and
`FrontendAdapter` into a single API-driven runtime loop with the interactive UI as a
consumer.

### Phase E — Structured Tools Hard Cutover

If structured tools remain the supported direction, remove
`src/state/conversation/tool_call_parser.rs` and the text-protocol fallback
path rather than preserving them as indefinite compatibility seams.

## Consequences

### Positive

- Replay and debug traces gain stable event identity and timestamp metadata.
- Tool execution becomes observable without transcript re-interpretation.
- The interactive UI, local API server, and peer orchestration can converge on one
  contract instead of parallel runtime flavors.
- Trait removal becomes safer because the event API becomes the stable seam.

### Negative

- `RuntimeEnvelope` and request schema changes are breaking for internal
  consumers that assumed the older minimal envelope.
- Event producers must emit richer metadata consistently, which raises the bar
  for tests and fixtures.
- The eventual removal of `RuntimeMode`, `FrontendAdapter`, and
  `ToolCallParser` will touch broad runtime surfaces and must be phased.

## Compliance

The following follow-on changes must conform to this amendment:

1. New internal runtime flow must prefer typed events over new UI-routing
   traits.
2. Tool lifecycle additions must be represented as explicit runtime events.
3. Envelope and request schema work must be treated as the prerequisite for
  removing legacy runtime-loop branching.