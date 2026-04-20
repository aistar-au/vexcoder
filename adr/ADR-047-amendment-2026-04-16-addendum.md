# ADR-047 Amendment Research Addendum: Protocol-Level Evidence for the API-First Pivot

**Date:** 2026-04-16
**Status:** Amended
**Supplements:** ADR-047, ADR-047-amendment-2026-04-16
**Purpose:** Document the open-specification evidence that supports the
decisions in the companion amendment, with full references.

---

## 1. Envelope Identity and Correlation: Open-Protocol Convergence

### 1.1 JSON-RPC 2.0 Request/Response ID Contract

The JSON-RPC 2.0 specification (2010, JSON-RPC Working Group) defines a
client-generated `id` field on every Request object. The specification states
that the id "MUST contain a String, Number, or NULL value" and that "the Server
MUST reply with the same value in the Response object." A Request without an
`id` is explicitly designated a Notification, which the specification says "the
Server MUST NOT reply to."

This establishes the foundational protocol-level precedent that streaming agent
runtimes inherit: the distinction between requests that expect replies (with an
id) and notifications that do not (without an id) is a first-class protocol
concern, not an implementation detail. Vexcoder's existing `RuntimeRequest`
schema lacked a client-generated request ID, making it impossible to correlate
a response envelope back to the request that triggered it.

**Relevance to ADR-047 amendment, Decision 5 (Runtime Requests Also Follow an
API Contract):** The request_id field added in Phase A directly implements the
JSON-RPC id semantics, and the notification vs. response distinction aligns
with the amendment's requirement for explicit interrupt/cancel/resume modeling.

**Protocol precedent:** JSON-RPC 2.0, Sections 4 and 4.1. The specification is
transport-agnostic and defines the id/notification split as a universal RPC
concern.

### 1.2 Language Server Protocol Progress Tokens

The Language Server Protocol (LSP) 3.17 specification extends JSON-RPC with a
`ProgressToken` type (integer or string) used to correlate `$/progress`
notifications back to the originating request. LSP defines a three-phase
progress lifecycle:

- `WorkDoneProgressBegin`: signals operation start, carries title and optional
  percentage.
- `WorkDoneProgressReport`: intermediate progress, carries optional message and
  percentage update.
- `WorkDoneProgressEnd`: signals completion, carries optional message.

This lifecycle is structurally identical to the tool-call lifecycle adopted in
the ADR-047 amendment: `tool_call_started` maps to Begin, `tool_call_arguments_delta`
maps to Report, and `tool_call_completed`/`tool_call_failed` map to End. The
key architectural insight is that LSP treats progress as a first-class protocol
event type rather than encoding it inside response payloads.

LSP also distinguishes between server-initiated and client-initiated progress,
and between partial results (streamed portions of a final result set) and work
done progress (human-oriented progress reporting). Both use the same
`ProgressToken` correlation mechanism.

**Relevance to ADR-047 amendment, Decision 3 (Tool Lifecycle Becomes an Explicit
API Contract):** The three-phase lifecycle validates the
started/delta/completed pattern rather than the older two-event (ToolCall
followed by ToolResult) pattern.

**Protocol precedent:** LSP 3.17, Sections "Progress Support," "Work Done
Progress," and "Partial Result Progress." The LSP base protocol layer is
defined over JSON-RPC 2.0 (see §1.1).

### 1.3 CloudEvents Identity Fields

The CloudEvents specification (v1.0.2, CNCF Serverless Working Group) defines
a minimal set of required attributes for any event:

- `id` (string, required): identifies the event; unique within the scope of
  the source.
- `source` (URI-reference, required): identifies the context in which the
  event happened.
- `type` (string, required): the type of event.
- `time` (timestamp, optional but recommended): when the occurrence happened.

The specification explicitly states that `id` plus `source` must be unique, and
that the `time` field "might be populated with [the] time when the occurrence
happened" or "when the event data was created."

Vexcoder's original `RuntimeEnvelope` used only a monotonic `seq` counter,
which is insufficient for CloudEvents-compatible identity: `seq` is not unique
across task boundaries, carries no temporal information, and provides no source
attribution for multi-agent scenarios.

**Relevance to ADR-047 amendment, Decision 2 (Envelope Metadata Must Carry
Identity, Time, and Correlation):** The `event_id`, `emitted_at`, and `source`
fields added in Phase A directly satisfy the CloudEvents core attribute
contract. The `parent_event_id` field extends the pattern for causal chaining,
which CloudEvents supports via extension attributes.

**Protocol precedent:** CloudEvents v1.0.2. The event identity model aligns
with RFC 4122 (UUID) for `id` generation, RFC 3986 (URI) for `source`, and
RFC 3339 for timestamps.

---

## 2. Tool Lifecycle Events: Explicit State Machine Over Transcript Inference

### 2.1 Chat Completions Streaming Tool Calls

The chat completions streaming API (widely adopted in the industry) emits tool
call invocations as structured objects within the `choices[].delta.tool_calls`
array. Each tool call carries:

- `index`: position in the tool call array for that turn.
- `id`: a unique identifier for the tool call, present only in the first
  chunk.
- `function.name`: present only in the first chunk.
- `function.arguments`: typically a partial JSON string, streamed
  incrementally across multiple chunks.

The critical design decision is that tool call identity (`id`) and tool name
are emitted once at the start of the tool call, while arguments are streamed as
incremental deltas. The consumer never needs to infer tool identity from
content blocks; it is an explicit protocol-level field.

When arguments are streamed as partial JSON, the consumer must buffer and
concatenate before parsing. Invalid or truncated JSON is a known failure mode
that requires explicit handling in the consumer.

For local-server interoperability, the ingress normalizer also accepts a fully
materialized JSON value in `function.arguments` and converts that variation to
the same typed runtime tool-call contract before the event leaves the API
boundary. This keeps the downstream CLI and ratatui layers independent of
server-specific serialization choices.

RFC 8259 is the standards basis for that split: arrays are ordered sequences,
so coalescence by `tool_calls[index]` is stable, while object members are
unordered and interoperable consumers must not depend on member order. The
normalizer therefore resolves transport framing and JSON-value assembly at
ingress rather than asking downstream consumers to preserve or reparse raw
member ordering.

Local runtime interoperability also exposed a transport-level variation:
servers that accept `stream = true` but do not emit an initial SSE event
promptly. The WHATWG SSE processing model only yields consumer-visible events
after the sender produces event fields and a frame terminator, so an accepted
connection with no early event provides no stream facts for downstream code to
consume. For local and private-network endpoints, the client now retries that
stalled start once with `stream = false` and runs the full JSON response
through the same ingress normalizer. The downstream API contract therefore
remains `RuntimeEnvelope`-only across both streamed and full-response local
variants.

Official local runtime API documents also record the two variations absorbed
here. Common local chat APIs document that `stream: false` can return a single
JSON object and show tool-call `arguments` as a materialized JSON object in
both streaming and non-streaming examples. Common local chat-completions
servers document both synchronous and streaming `/v1/chat/completions`, parsed
tool-call support, reasoning-content fields, and capability-discovery
endpoints. Together these sources show that local runtimes legitimately vary
in transport mode and argument materialization while still advertising stable
APIs, which is why the consumer-facing contract must remain the normalized
runtime envelope rather than raw provider payloads.

**Relevance to ADR-047 amendment, Decision 3:** The streaming tool call pattern
directly validates the `tool_call_started` (carries id and name) followed by
`tool_call_arguments_delta` (carries partial argument string) followed by
completion/failure design. The older Vexcoder approach of encoding tool
arguments inside `TranscriptBlockDelta` events forced downstream consumers to
infer tool identity from renderer-oriented content blocks.

### 2.2 MCP Tool Invocation Model

The Model Context Protocol (MCP, 2025-03-26 specification) defines tool
invocation via JSON-RPC method calls. A tool call is a standard JSON-RPC
request with:

- `method`: `"tools/call"`
- `params.name`: the tool name.
- `params.arguments`: structured arguments as a JSON object.
- `id`: the JSON-RPC request id for correlation.

The response is a standard JSON-RPC response with `content` (an array of text
or image blocks), `isError` (boolean), and the correlated `id`. MCP treats
tool execution as a synchronous request/response within the RPC layer; there
is no separate event for tool start and tool end. The lifecycle is implicit in
the request/response boundary.

This is a simpler model than streaming tool lifecycle events, but it confirms
the pattern of using structured protocol fields (method, name, arguments, id)
rather than embedding tool semantics in transcript content.

**Relevance to ADR-047 amendment, Decision 3 and 4:** MCP validates that tool
identity and arguments belong in the protocol layer, not in content blocks.
The amendment goes further than MCP by adding explicit lifecycle events for
streaming scenarios where tool execution is long-running.

**Protocol precedent:** Model Context Protocol 2025-03-26, Section
"Server: Tools." The tool invocation model is defined over JSON-RPC 2.0
(see §1.1).

---

## 3. Request Correlation and Notification Semantics

### 3.1 JSON-RPC Notification Contract

JSON-RPC 2.0 defines a Notification as "a Request object without an 'id'
member" and specifies that "the Server MUST NOT reply to a Notification,
including those that are within a batch request." Furthermore, "Notifications
are not confirmable by definition, since they do not have a Response object to
be returned."

This creates a clean dichotomy: messages with an id expect a correlated
response; messages without an id are fire-and-forget. Vexcoder's existing
request schema mixed these semantics: `submit`, `interrupt`, `approve`, and
`deny` all used the same request structure without distinguishing which expect
responses and which are notifications.

**Relevance to ADR-047 amendment, Decision 5:** The request_id addition enables
the JSON-RPC-style distinction. Requests that carry a request_id expect
correlated response envelopes. Pure notifications (like interrupt signals)
can be modeled without a request_id.

### 3.2 LSP Cancellation and Structured Error Codes

LSP defines a `$/cancelRequest` notification that carries the `id` of the
request to cancel. This is a protocol-level cancellation mechanism: the client
sends a notification referencing the original request id, and the server is
expected to return an error response with code `-32800` (RequestCancelled).

JSON-RPC also defines structured error codes in the `-32768` to `-32000`
reserved range: Parse error (-32700), Invalid Request (-32600), Method not
found (-32601), Invalid params (-32602), and Internal error (-32603), with
`-32000` to `-32099` reserved for server-defined errors.

**Relevance to ADR-047 amendment, Decision 5:** Structured cancel semantics
require a request_id to reference. The amendment's requirement for "explicit
cancel or resume semantics" is directly supported by the LSP precedent of
cancellation-as-notification-referencing-original-id.

---

## 4. UI as Consumer, Not Runtime Flavor: Architectural Precedent

### 4.1 The Single-Stream Consumer Pattern

Both LSP and JSON-RPC define a single bidirectional message stream between
client and server. The server does not maintain separate "transport modes" for
different consumers; all consumers (editors, debuggers, language services)
receive the same protocol messages and filter locally.

LSP's capability negotiation (via `InitializeResult.capabilities`) allows the
server to advertise which features it supports, but the message format is
identical regardless of the consumer type. There is no `EditorMode` or
`DebuggerMode` trait; the protocol itself is the shared surface.

**Relevance to ADR-047 amendment, Decisions 1 and 6:** The existing
`RuntimeMode` and `FrontendAdapter` traits in Vexcoder represent the opposite
pattern: separate runtime flavors for different consumers. The amendment's
decision to treat the interactive UI as a consumer of the runtime API (rather
than a separate runtime flavor) aligns with the LSP/JSON-RPC single-stream
pattern.

### 4.2 Trait Boundaries for External Systems Only

In the protocols reviewed, abstraction boundaries (interfaces, traits) are
placed at external system boundaries: language backends, file system access,
build systems, transport layers. Internal routing between features within the
same process is handled by typed messages and pattern matching, not by trait
dispatch.

**Relevance to ADR-047 amendment, Decision 7 (True External Boundary Traits
Remain Acceptable):** The amendment's keep-list (model backend, sandbox
driver, approval policy, command runner) represents genuine external-system
boundaries. The follow-up simplification list (runtime mode, frontend adapter,
tool call parser) represents internal routing that the API event surface
replaces.

---

## 5. Phasing Validation: What Phase A Implements

Phase A of the amendment adds:

1. **Envelope metadata:** `event_id` (formatted as
   `evt:{task_id}:{turn}:{seq}`), `emitted_at` (ISO 8601), `source` (Model,
   Runtime, or UserRequest), `request_id`, and `parent_event_id` (referencing
   another `event_id` in the same format) to `RuntimeEnvelope`.

2. **Explicit tool lifecycle events:** `ToolCallStarted`,
   `ToolCallArgumentsDelta`, `ToolCallCompleted`, and `ToolCallFailed` replace
   the single `ToolCall`/`ToolResult` pair.

3. **Source attribution refinement:** `TranscriptLine` events are attributed
   to `Runtime`; `TranscriptBlockStart` events delegate to a block-type-aware
   function that classifies `ToolResult` blocks as `Runtime` and other blocks
   as `Model`. Block delta and complete events inherit the tracked source from
   their opening start event.

4. **O(1) tool context lookup:** A `PendingToolCallContext` struct keyed by
   runtime tool-call ID provides constant-time metadata access during
   argument-delta recording, replacing the previous linear scan.

5. **SSE consumer migration:** The SSE mapper was updated from the removed
   `ToolCall`/`ToolResult` variants to the new lifecycle event variants.

6. **JSON schemas:** `schemas/runtime_envelope_v1.json` and
   `schemas/runtime_request_v1.json` codify the new envelope and request
   contracts.

### Test coverage

- Source attribution assertions added to the existing normalization test.
- `test_pi_10_runtime_origin_block_sources_are_preserved` verifies
  `ToolResult` stream blocks receive `RuntimeEnvelopeSource::Runtime`.
- Focused normalization and library tests passed on the implementation branch.

---

## References

1. RFC 8259 — The JavaScript Object Notation (JSON) Data Interchange Format.
   https://www.rfc-editor.org/rfc/rfc8259

2. RFC 4122 — A Universally Unique IDentifier (UUID) URN Namespace.
   https://www.rfc-editor.org/rfc/rfc4122

3. RFC 3986 — Uniform Resource Identifier (URI): Generic Syntax.
   https://www.rfc-editor.org/rfc/rfc3986

4. RFC 3339 — Date and Time on the Internet: Timestamps.
   https://www.rfc-editor.org/rfc/rfc3339

5. RFC 9110 — HTTP Semantics.
   https://www.rfc-editor.org/rfc/rfc9110

6. RFC 9111 — HTTP Caching.
   https://www.rfc-editor.org/rfc/rfc9111

7. RFC 9112 — HTTP/1.1.
   https://www.rfc-editor.org/rfc/rfc9112

8. WHATWG HTML Living Standard — Server-sent events.
   https://html.spec.whatwg.org/multipage/server-sent-events.html
9. RFC 8446 — The Transport Layer Security (TLS) Protocol Version 1.3.
   https://www.rfc-editor.org/rfc/rfc8446
