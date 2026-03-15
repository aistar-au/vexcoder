# ADR-025: Runtime JSON handoff contract — trait-level event envelopes, JSON projection, and adapter-stable serialization

**Date:** 2026-03-11
**Status:** Proposed
**Deciders:** Core maintainer
**Location:** `adr/ADR-025-runtime-json-handoff-contract.md`
**ADR chain:** ADR-024 (Phase I reservation), ADR-023 (deterministic edit loop), ADR-006 (runtime mode contracts)
**Related:** `src/types/api_types.rs`, `src/batch_mode.rs`

---

## Context

ADR-006 established the runtime seam around `RuntimeMode`, `RuntimeContext`, `UiUpdate`, and `FrontendAdapter`. That seam is correct for in-process Rust execution, but it does not yet define a **canonical machine-readable handoff** for the same events.

Today the codebase has two JSON-shaped surfaces, but no shared contract between them:

1. Provider-facing API message and stream types in `src/types/api_types.rs`.
2. Batch/headless JSONL evidence output in `src/batch_mode.rs`.

Those two surfaces serve different jobs and are both valid, but neither is the correct architectural center for a future API-facing runtime:

- provider stream types are backend-specific and should not leak across the runtime seam;
- BatchMode JSONL is an append-only output format, not a transport-neutral trait contract.

ADR-024 reserves `LocalApiServer` as a later `RuntimeMode + FrontendAdapter` path and requires a dedicated ADR for its wire protocol, authentication model, and streaming response format. Before that transport ADR can be correct, the runtime needs a single **serde-stable JSON contract** that represents runtime requests and runtime events independently of transport.

The missing decision is therefore not "SSE first" or "JSONL first." The missing decision is:

> what JSON contract exists between runtime traits so the same runtime can later be projected as a local API without duplicating logic or inventing a second event model.

That seam is broader than a future HTTP server. In milestone 1 it must already support CLI/TUI execution and BatchMode evidence output, and later it must support `LocalApiServer`, task handoff, and other JSON-capable adapters without introducing a second contract. Browser-specific origin policy, web-UI behavior, and multi-agent queue ownership rules remain separate concerns; this ADR defines only the canonical request/event seam they would consume.

**Checklist continuation note:** ADR-024 Phase I checklist items PI-01 through PI-08 cover session lifecycle and command-surface work (`/permissions`, `/allow`, `/deny`, `/new`, `/resume`, `/mcp list`, `/mcp show`, `/plan`/`/context`). **Note:** PI-08 (`/plan` and `/context`) is tracked in ADR-023 EL-11/EL-12 and is only listed in ADR-024 for cross-reference. This ADR extends the Phase I checklist from PI-09 through PI-12. ADR-026 continues from PI-13 through PI-16. ADR-028 defines the application-facade and transport-boundary rule that later CLI and server work must respect. A reconciliation change must keep ADR-024's Phase I checklist and config-key section aligned with ADR-025 and ADR-026 before transport work is treated as merge-ready.

**Current codebase naming note:** the live repository currently contains both provider-facing `ContentBlock::ToolUse { id }` / `ToolResult { tool_use_id, content, is_error }` types in `src/types/api_types.rs` and runtime-facing tool event names such as `StreamBlock::ToolCall` / `ToolResult { tool_call_id, output, is_error }` in the internal streaming path. This ADR does not require those existing names to unify immediately. PI-10 is the normalization layer that maps both existing shapes into one canonical runtime JSON contract.

---

## Sequencing guard

This ADR satisfies ADR-024's Phase I specification requirement, but implementation must not begin until:

1. Phase H (macOS packaging and distribution) is complete, and
2. milestone-1 correctness work (ADR-022 phases 1–8 plus ADR-023 deterministic edit loop) is validated end-to-end.

No dispatcher may begin canonical JSON handoff implementation before that gate is green.

---

## Decision

Introduce a canonical, transport-neutral, serde-backed JSON handoff layer for the runtime seam.

### 1. Canonical machine-readable contract

Add a new runtime-level envelope model:

```rust
// src/runtime/json_handoff.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeEnvelope {
    pub version: u16,          // fixed at 1 for this ADR
    pub task_id: String,
    pub turn: u32,
    /// Resets to 1 at each new turn. Monotonically increasing within a turn.
    /// Never decreases within a turn. Clients may use this to detect dropped
    /// or duplicated envelopes in streaming transports.
    pub seq: u64,
    pub event: RuntimeEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    TurnStart {
        input: Option<String>,
    },
    AssistantDelta {
        text: String,
    },
    AssistantMessage {
        content: String,
    },
    ToolCall {
        /// Generated by runtime code in the format `call_<utc-ms>_<4-hex-random>`.
        /// The model emits only `name` and `arguments`; the runtime injects `id`
        /// during normalization (PI-10). The id must be unique within a session.
        id: String,
        name: String,
        arguments: serde_json::Value, // schema-constrained to an object at the API boundary
    },
    ToolResult {
        tool_call_id: String,
        tool_name: Option<String>,
        is_error: bool,
        output: String,
    },
    ApprovalRequest {
        capability: String,
        scope: String,
        tool_name: Option<String>,
    },
    ApprovalResolved {
        capability: String,
        scope: String,
        approved: bool,
    },
    ValidationResult {
        passed: bool,
        outputs: Vec<ValidationOutputEnvelope>,
    },
    TurnEnd {
        /// "completed" — turn ran to natural completion.
        /// "failed"    — turn terminated due to a non-recoverable runtime error;
        ///               a non-recoverable Error envelope precedes this.
        /// "cancelled" — turn was interrupted by RuntimeRequest::Interrupt or
        ///               user-initiated cancellation.
        status: String,
        usage: Option<TokenUsageEnvelope>,
        changed_files: Vec<String>,
    },
    Error {
        code: String,
        message: String,
        recoverable: bool,
    },
    MaxTurnsReached {
        max_turns: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationOutputEnvelope {
    pub label: String,
    pub exit_code: i32,
    pub stdout_tail: String,
    pub stderr_tail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageEnvelope {
    pub input: u64,
    pub output: u64,
    #[serde(default)]
    pub estimated: bool,
}
```

This envelope model is the **single source of truth** for machine-readable runtime handoff.

**Tool ID ownership rule (normative):** The model backend emits `name` and `arguments` only. The runtime normalization layer (PI-10) injects a unique `id` in the format `call_<utc-ms>_<4_hex_random>` (for example `call_1741700123456_9a2f`). No model backend or provider adapter may generate or assume the value of `ToolCall.id`; id generation is exclusively a runtime responsibility. The id must be unique within a session.

This rule resolves the ambiguity that existed when provider-facing types (e.g. `ContentBlock::ToolUse { id }`) appeared to carry a model-originated id. Those ids are provider-protocol artifacts; PI-10 normalization replaces them with a runtime-generated id before the event reaches the canonical layer.

**`MaxTurnsReached` event:** emitted as the terminal event for a turn when `BatchMode` or `LocalApiServer` exhausts `--max-turns`. A `TurnEnd { status: "failed" }` follows immediately. This allows clients to distinguish normal completion from limit-triggered termination without inspecting exit codes. `MaxTurnsReached` is a first-class event in the canonical layer, not a transport side-channel.

**Usage deferral note:** `TurnEnd.usage` remains optional until ADR-024 Gap 28 / PL-03 lands. Before that point, omission is valid and expected. This ADR defines the JSON slot now so later transport/API work does not need a schema-breaking change.

### 2. Normalization mapping is explicit

PI-10 is the canonical normalization layer. It must map provider-facing and current runtime-facing shapes into the envelope model above.

**Normalization mapping (PI-10):**

| Source shape | Canonical envelope | Mapping rule |
|--------------|--------------------|--------------|
| `ContentBlock::ToolUse { id, name, input }` | `ToolCall { id, name, arguments }` | **Runtime discards provider id and generates a new `call_<utc-ms>_<4-hex>` id**; `input` becomes `arguments` |
| `ContentBlock::ToolResult { tool_use_id, content, is_error }` | `ToolResult { tool_call_id, tool_name, is_error, output }` | `tool_use_id` is looked up in the pending-call table to find the runtime-generated `call_*` id; that runtime id becomes `tool_call_id`; `content -> output`; `is_error` pass-through; `tool_name` resolved from the pending-call table when available |
| `StreamBlock::ToolCall { id, name, input }` | `ToolCall { id, name, arguments }` | **Runtime discards any provider id and generates a new `call_<utc-ms>_<4-hex>` id**; `input` becomes `arguments` |
| `StreamBlock::ToolResult { tool_call_id, output, is_error }` | `ToolResult { tool_call_id, tool_name, is_error, output }` | `tool_call_id` is re-keyed to the runtime-generated id for the matching call; `output` and `is_error` pass-through; `tool_name` resolved from the pending-call table when available |
| `UiUpdate::StreamDelta(text)` | `AssistantDelta { text }` | direct pass-through |
| `UiUpdate::TurnComplete` | `AssistantMessage { content }` then `TurnEnd { status: "completed", ... }` | the normalization layer accumulates all `AssistantDelta.text` emitted for the turn and, on `TurnComplete`, emits a single `AssistantMessage { content: accumulated_text }` immediately before the terminal `TurnEnd` |
| `UiUpdate::Error(message)` | `Error { code, message, recoverable }` then `TurnEnd { status: "failed", ... }` when possible | normalized failure path |
| `UiUpdate::ToolApprovalRequest(req)` | `ApprovalRequest { capability, scope, tool_name }` | extract fields from `ToolApprovalRequest`; `tool_name` resolved from pending-call table when available |
| `RuntimeRequest::ApproveCapability` processed | `ApprovalResolved { capability, scope, approved: true }` | emitted after the runtime processes an approval grant; precedes the resumed `ToolCall` |
| `RuntimeRequest::DenyCapability` processed | `ApprovalResolved { capability, scope, approved: false }` | emitted after the runtime processes an approval denial |
| `UiUpdate::StreamBlockStart`, `StreamBlockDelta`, `StreamBlockComplete` | *(not projected)* | TUI render bookkeeping only; must not cross the machine-readable seam |
| `BatchMode` / `LocalApiServer` max-turns limit reached | `MaxTurnsReached { max_turns }` then `TurnEnd { status: "failed", ... }` | emitted as the terminal sequence when the turn limit is exhausted |
| Grammar `tool_call_array` (array of tool calls) | one `ToolCall` envelope per array element | when the grammar produces `[{...},{...}]`, PI-10 splits the array into individual `ToolCall` envelopes, each with its own runtime-generated `id` and its own `seq` number; single bare `{...}` objects are treated as a one-element array |

**Current-path note:** the live `UiUpdate` surface does not yet contain a dedicated `AssistantMessage` variant. PI-10 therefore treats the `TurnComplete` path above as the normative source of `AssistantMessage` for current streaming backends: deltas are accumulated across the turn, a terminal `AssistantMessage` is emitted immediately before `TurnEnd`, and BatchMode derives `TurnRecord.response` from that assembled content when present. A future non-streaming backend may add a direct full-message update, but it must normalize to the same `AssistantMessage` envelope shape.

**Validation integration note:** `ValidationOutputEnvelope` is the API-facing projection of ADR-023 `ValidationSuite` output. Its `label` field corresponds to validation command names such as `cargo test`, `cargo clippy`, or `npm test`. This gives API clients structured validation data without coupling them to ADR-023's internal Rust types.

**Implementation note (PF-01 / PF-02 dependency):** `ToolResult.tool_name` remains `Option<String>` because ADR-024 PF-01 / PF-02 (`McpRegistry` and `Capability::McpTool` approval wiring) are not yet green in the live roadmap. The PI-10 normalization layer must resolve `tool_name` from the pending-call table when that name is available. When it is not available — especially for future MCP-originated tool results before registry wiring is complete — the canonical envelope must emit `tool_name = null` rather than inventing a placeholder string.

### 3. Transport neutrality is mandatory

`RuntimeEnvelope` is the canonical contract. JSONL, SSE, Unix-socket streams, files, and tests are all **serializations or projections** of that contract.

This ADR explicitly makes the following distinction normative:

- **Canonical layer:** typed Rust structs/enums + serde JSON shape
- **Serialization layer:** JSON object, JSONL line, SSE `data:` payload, future WebSocket message
- **Provider layer:** upstream/vendor stream formats, which must be normalized before they reach the canonical layer

JSONL is therefore **not** the runtime's source of truth. It is one output mode derived from the canonical envelope model.

### 4. Trait-level handoff uses envelopeable events

ADR-006's `UiUpdate` and `UserInputEvent` remain valid in-process runtime types. This ADR does **not** replace those trait signatures immediately. Instead it introduces an additive rule:

- any runtime event that crosses a machine-readable seam must have a deterministic projection into `RuntimeEnvelope` or `RuntimeRequest`;
- any new API-facing adapter must use those projections rather than inventing a parallel event schema.

The request side is defined as:

```rust
// src/runtime/json_handoff.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeRequest {
    SubmitInput {
        task_id: Option<String>,
        input: String,
    },
    Interrupt {
        task_id: String,
    },
    /// Grants the named capability in the current session.
    /// Emitted by the client when the operator approves a pending ApprovalRequest.
    ApproveCapability {
        task_id: String,
        capability: String,
        /// Uses the same vocabulary as ApprovalRequest/ApprovalResolved: "once" | "session".
        scope: String,
    },
    /// Denies the named capability in the current session.
    /// Emitted by the client when the operator denies a pending ApprovalRequest.
    DenyCapability {
        task_id: String,
        capability: String,
    },
}
```

This makes the runtime seam bidirectional in JSON terms:

- client/adapters send `RuntimeRequest`
- runtime emits `RuntimeEnvelope`

Approval scope vocabulary is fixed across the canonical seam: `ApprovalRequest.scope`, `ApprovalResolved.scope`, and `RuntimeRequest::ApproveCapability.scope` use the same `"once"` / `"session"` value set.

### 5. Tool-call grammar is part of the contract surface

For model backends that support constrained decoding, tool-call emission must be constrained by a grammar file:

```text
grammars/tool_call.gbnf
```

This grammar is normative for the JSON shape of `ToolCall` payloads generated by a model backend. It is not a transport-binding concern and does not belong in ADR-026. LocalApiServer consumes already-normalized runtime events; it does not define model-decoding rules.

Rules:

- built-in tool names must be explicitly enumerated;
- MCP tools must use the `mcp.<server>.<tool>` namespace;
- the grammar must admit valid JSON objects only;
- backends that do not support constrained decoding may still emit tool calls, but PI-10 normalization and serde/schema validation remain mandatory.

The full grammar file is included in Appendix A of this ADR.

**Clarification (ADR-024 Gap 11):** ADR-024 Gap 11's `tool_call_mode = "structured"` refers to provider API structured-output features on hosted backends. This ADR's GBNF grammar covers local-model constrained decoding for backends that do not expose provider-native structured tool calling. Both mechanisms target the same outcome — valid JSON tool-call payloads — but at different layers. PI-10 normalization must accept both paths and produce identical `RuntimeEnvelope` output regardless of backend type.

**Note on GBNF `json_safe_char`:** The grammar in Appendix A defines a conservative `json_safe_char` set for local-model constrained decoding. This set intentionally excludes some characters valid in JSON strings (e.g. `(`, `)`, `;`, `$`) to limit the local-model output surface. This restriction applies to unescaped characters only; standard JSON escapes, including `\uXXXX`, remain valid through the `escape` rule. Backends that do not use constrained decoding are not restricted by this grammar; PI-10 normalization and serde validation accept the full JSON string character set from those backends.

### 6. BatchMode remains backward-compatible and derivable

Existing `vex exec --format jsonl` output remains unchanged for compatibility.

`TurnRecord` and `SummaryRecord` stay the operator-facing JSONL format for BatchMode.

However, ADR-025 imposes a new internal rule:

- BatchMode JSONL must be derivable from the canonical `RuntimeEnvelope` stream without inventing hidden fields or semantics.

**BatchMode derivation rules:**

- `TurnRecord.input` is the `TurnStart.input` for the corresponding turn.
- `TurnRecord.response` is the `AssistantMessage.content` value when that terminal message event is emitted; otherwise it is the concatenation of all `AssistantDelta.text` values in order. `AssistantMessage.content` is the authoritative full-turn response string and supersedes delta concatenation when present.
- `TurnRecord.changed_files` is copied from `TurnEnd.changed_files`.
- `SummaryRecord.status` is copied from the final `TurnEnd.status`.
- `SummaryRecord.total_turns` equals the count of completed turns.
- command-history evidence recorded in BatchMode must be traceable to the canonical tool/validation event stream for that turn; PI-12 tests must prove that replaying canonical envelopes reconstructs the existing summarized JSONL shape.
- When `MaxTurnsReached` is emitted, `SummaryRecord.status` is `"failed"` and a `max_turns_reached: true` field is added to the summary record. This is additive and does not break existing tooling that reads only `status`.

A future additive format such as `--format json-events` may emit one `RuntimeEnvelope` per line, but this ADR does not require that flag to exist now.

### 7. Schema v1 is required

Add versioned schema files:

```text
schemas/runtime_envelope_v1.json
schemas/runtime_request_v1.json
```

Rules:

- schema version is locked to `version = 1` in this ADR;
- all envelope and request types must round-trip through serde JSON;
- CI must verify schema-generation parity and round-trip stability;
- MCP namespace validation must exist both in the grammar and in the JSON Schema pattern for `ToolCall.name`.

The full envelope schema is included in Appendix B of this ADR. A compact request schema is included in Appendix C.

**Schema versioning policy:** `version = 1` is locked for this ADR. A future ADR that changes the shape of any `RuntimeEnvelope` or `RuntimeRequest` type must bump `version` to `2` or higher. The version field is monotonically increasing and must never decrease. Backward-incompatible field additions, renames, or removals require a version bump and a migration note in the new ADR.

### 8. Correlation, ordering, and recovery rules

The canonical envelope model must obey these rules:

- `task_id` is present on every envelope;
- `turn` is present on every envelope;
- `seq` resets to `1` at the start of each new turn;
- `seq` must never decrease within a turn and is monotonically increasing within that turn;
- clients may use `seq` to detect dropped or duplicated envelopes in streaming transports;
- `ToolResult.tool_call_id` must match the runtime-generated `ToolCall.id` for the matching call in the same task;
- `TurnEnd` is the terminal event for a turn unless the transport itself fails before completion.
- PI-12 tests must assert that the first envelope of every turn has `seq == 1`.

**Error recovery behavior:**

- `Error.recoverable = true`: the turn continues. The runtime may emit additional `AssistantDelta`, `ToolCall`, `ToolResult`, `ValidationResult`, or other in-turn envelopes after the error. No automatic `TurnEnd` is required solely because the error occurred.
- `Error.recoverable = false`: the turn terminates. A `TurnEnd` with `status: "failed"` must follow as soon as runtime state permits; if that cannot be emitted, the adapter must record transport termination for the affected turn.

### 9. Out of scope for this ADR

This ADR does **not** define:

- HTTP routes
- SSE binding details
- Unix-socket path or permissions
- bearer authentication
- `/v1/schema` endpoint shape
- WebSocket support
- `vex remote-control` or external network exposure

Those belong to ADR-026, the transport-binding ADR that follows this one.

---

## Example canonical envelopes

```json
{"version":1,"task_id":"task-1741700000000","turn":1,"seq":1,"event":{"type":"turn_start","input":"review src/app.rs"}}
{"version":1,"task_id":"task-1741700000000","turn":1,"seq":2,"event":{"type":"assistant_delta","text":"Analyzing current runtime flow..."}}
{"version":1,"task_id":"task-1741700000000","turn":1,"seq":3,"event":{"type":"tool_call","id":"call_1741700123456_9a2f","name":"read_file","arguments":{"path":"src/app.rs"}}}
{"version":1,"task_id":"task-1741700000000","turn":1,"seq":4,"event":{"type":"tool_result","tool_call_id":"call_1741700123456_9a2f","tool_name":"read_file","is_error":false,"output":"..."}}
{"version":1,"task_id":"task-1741700000000","turn":1,"seq":5,"event":{"type":"assistant_message","content":"Analyzing current runtime flow..."}}
{"version":1,"task_id":"task-1741700000000","turn":1,"seq":6,"event":{"type":"turn_end","status":"completed","usage":{"input":184,"output":67,"estimated":false},"changed_files":[]}}
```

---

## Integration with existing ADRs

| Existing ADR item | ADR-025 decision |
|-------------------|------------------|
| ADR-006 runtime seam | Adds canonical JSON projection for the seam without replacing in-process Rust trait signatures immediately |
| ADR-023 deterministic edit loop | Makes edit-loop, validation, and tool events serializable for later API adapters |
| ADR-024 Gap 2 (BatchMode) | Preserves current BatchMode JSONL compatibility while making it derivable from canonical envelopes |
| ADR-024 Gap 5 (MCP) | `ToolCall.name` supports and validates the `mcp.<server>.<tool>` namespace |
| ADR-024 Gap 28 (Token counter) | `TurnEnd.usage` carries optional `TokenUsageEnvelope` with `estimated`, but remains optional until PL-03 is green |
| ADR-024 Phase I reservation | Supplies the missing transport-neutral contract that Phase I needs before a wire protocol can be bound |

---

## Rationale

### Why does the runtime own tool-call id generation?

Provider-facing `ContentBlock::ToolUse { id }` carries a provider-assigned id. That id is scoped to the provider protocol and is not guaranteed to be unique across sessions, across providers, or in local/offline model backends that do not assign ids at all. Runtime-generated ids in the format `call_<utc-ms>_<4-hex-random>` are:

- generated by one code path regardless of backend type;
- unique within a session by construction;
- stable for `ToolResult.tool_call_id` correlation regardless of which provider or model is active.

Provider ids remain available inside the provider adapter for protocol-level correlation, but they do not cross the canonical layer.

### Why not make JSONL the canonical contract?

JSONL is excellent for append-only logs, file exports, and line-oriented streaming. It is not the correct abstraction for trait boundaries because traits exchange **typed values**, not lines.

The runtime needs a canonical event model that can be serialized as:

- an in-memory JSON value
- a JSONL line
- an SSE payload
- a future socket frame

If JSONL were made the source of truth, every in-process consumer would be forced to think in terms of string framing rather than structured events.

### Why not reuse `src/types/api_types.rs` as the canonical contract?

Those types are backend/protocol-facing. They are appropriate for API adapters that speak a specific upstream protocol, but they are the wrong abstraction for the runtime seam because they would leak provider semantics into BatchMode, tests, and future LocalApiServer adapters.

The normalization layer exists precisely so that provider-native types can remain provider-native while the runtime seam stays stable.

### Why keep `task_id` schema-constrained only as a non-empty string?

Current code paths already use multiple task-id formats (`task-...` and `batch-...`). The canonical contract therefore validates only that `task_id` is a non-empty string. A stricter pattern would be inaccurate against current code and would create a false incompatibility between existing runtime surfaces.

### Why include the grammar appendix in the contract ADR?

The grammar constrains the **shape of the runtime tool-call JSON contract** at the model boundary. It is not a transport decision. The HTTP/SSE/socket binding ADR should stay thin and should not become the place where canonical event shapes are defined.

### Why include `ApproveCapability` and `DenyCapability` in `RuntimeRequest`?

`ApprovalResolved` is a first-class event in `RuntimeEvent`. For the canonical seam to be complete and bidirectional, the request side must include the signals that drive that event. A `RuntimeRequest` that can only submit input and interrupt turns cannot drive the approval flow from an external client. `ApproveCapability` and `DenyCapability` are therefore included in the canonical request schema even though approval in Phase I is primarily driven from the TUI. External clients that need to automate approval decisions have a defined path; TUI-driven approval continues to work identically via the existing `UiUpdate::ToolApprovalRequest` path.

---

## Consequences

**Easier after this ADR:**

- BatchMode, tests, exports, and a future LocalApiServer can all derive from one event model.
- The runtime can later "act as an API" without inventing a second schema unrelated to the in-process seam.
- Provider/backend streaming differences are isolated to normalization code.
- JSON tooling, schema validation, and contract tests become possible without coupling to transport details.

**Harder or more complex:**

- New runtime events now need a canonical envelope mapping and schema update.
- Provider adapters must perform explicit normalization rather than leaking native chunks forward.
- BatchMode compatibility tests must now assert that its JSONL remains derivable from the canonical event stream.
- Model backends that support constrained decoding need a maintained grammar file that stays in sync with tool-call shapes.

**Constraints imposed on future work:**

- No new API adapter may invent a parallel event schema instead of using `RuntimeEnvelope`.
- Future task-handoff, worker, or multi-agent surfaces must serialize `RuntimeRequest` / `RuntimeEnvelope` (or an explicitly versioned successor) rather than inventing a separate JSON dialect.
- No frontend or transport layer may consume provider-native event chunks directly.
- Browser-specific origin policy, CORS behavior, and web-UI interaction rules remain out of scope for this ADR and require a later transport/UI ADR.
- `schemas/runtime_envelope_v1.json`, `schemas/runtime_request_v1.json`, and `grammars/tool_call.gbnf` must stay in sync with Rust types and normalization rules.
- Existing BatchMode JSONL remains stable unless a separate ADR authorizes a breaking change.
- Transport details for LocalApiServer remain defined only in ADR-026.
- `ToolCall.id` is always generated by the runtime normalization layer. No model backend or provider adapter may generate or assume this value.
- `TurnEnd.status` values are: `"completed"`, `"failed"`, `"cancelled"`. No other values are valid. Additions require a schema version bump.
- `MaxTurnsReached` must always be followed by `TurnEnd { status: "failed" }`. These two events are always emitted as a pair.

---

## Dispatcher checklist

| ID | Task | Status |
|----|------|--------|
| **PI-09** | Add `src/runtime/json_handoff.rs` with `RuntimeRequest` (including `ApproveCapability` and `DenyCapability`), `RuntimeEnvelope`, `RuntimeEvent` (including `MaxTurnsReached`), `TokenUsageEnvelope`, `ValidationOutputEnvelope`, and `grammars/tool_call.gbnf` | [x] |
| **PI-10** | Add normalization layer from provider/native stream updates into canonical runtime envelopes; runtime injects `ToolCall.id`; provider ids are discarded at normalization boundary; include `UiUpdate::ToolApprovalRequest` → `ApprovalRequest` mapping; include `ApproveCapability`/`DenyCapability` → `ApprovalResolved` mapping; include `StreamBlockStart/Delta/Complete` explicit no-project rule | [ ] |
| **PI-11** | Add `schemas/runtime_envelope_v1.json` and `schemas/runtime_request_v1.json`, including `MaxTurnsReached` event, `ApproveCapability`/`DenyCapability` request variants, `tool_name` via `$ref` in `tool_result` (not inlined), and MCP namespace validation for `ToolCall.name` | [x] |
| **PI-12** | Add serde round-trip tests, schema parity tests, grammar parity tests, and BatchMode derivation tests. Tests must prove: first envelope of every turn has `seq == 1`; `TurnRecord` + `SummaryRecord` replay from canonical envelopes matches the existing JSONL shape modulo JSON field ordering; `TurnRecord.response` uses `AssistantMessage.content` when present and falls back to concatenated `AssistantDelta.text`; `TurnRecord.changed_files` matches `turn_end.changed_files`; `SummaryRecord.status` matches final `turn_end.status`; recoverable vs non-recoverable `error` envelopes follow the ordering rules in this ADR; `MaxTurnsReached` is always followed by `TurnEnd { status: "failed" }` | [ ] |

---

## Dispatcher reporting contract

When checking any PI-09…PI-12 box, append an evidence block:

```markdown
### [PI-XX] - <short title>
- Dispatcher: <name/id>
- Commit: <sha>
- Files changed:
  - `path/to/file` (+X -Y)
- Validation:
  - `cargo test --all-targets` : pass
  - `bash scripts/check_no_alternate_routing.sh` : pass
  - `bash scripts/check_forbidden_imports.sh` : pass
- Notes:
  - <what was built and why>
```

---

### [PI-09] - Canonical runtime handoff types and grammar
- Dispatcher: `dispatcher/adr-025-phase-1-kickoff`
- Commit: `a7b22137f779fd617b3ec1420b9a3a615e719fc0`
- Files changed:
  - `src/runtime.rs` (+4 -0)
  - `src/runtime/json_handoff.rs` (+169 -0)
  - `grammars/tool_call.gbnf` (+66 -0)
- Validation:
  - `cargo test --all-targets` : pass
  - `make gate-fast` : pass
  - `bash scripts/check_no_alternate_routing.sh` : pass
  - `bash scripts/check_forbidden_imports.sh` : pass
  - `bash scripts/check_forbidden_names.sh` : pass
- Notes:
  - Added the canonical ADR-025 Rust handoff surface and the normative tool-call grammar without starting the PI-10 normalization layer.
  - Keeps `ToolCall.id` ownership in the runtime contract while leaving provider-id discard and event projection work dependency-sequenced for PI-10.

### [PI-11] - Runtime envelope and request schemas
- Dispatcher: `dispatcher/adr-025-phase-1-kickoff`
- Commit: `a7b22137f779fd617b3ec1420b9a3a615e719fc0`
- Files changed:
  - `schemas/runtime_envelope_v1.json` (+193 -0)
  - `schemas/runtime_request_v1.json` (+53 -0)
- Validation:
  - `cargo test --all-targets` : pass
  - `make gate-fast` : pass
  - `bash scripts/check_no_alternate_routing.sh` : pass
  - `bash scripts/check_forbidden_imports.sh` : pass
  - `bash scripts/check_forbidden_names.sh` : pass
- Notes:
  - Added the versioned ADR-025 schema assets, including `MaxTurnsReached`, approval request variants, and MCP namespace validation for canonical tool names.
  - Leaves PI-12 schema/serde/grammar parity enforcement and BatchMode-derivation coverage sequenced behind PI-10.

## Compliance notes for agents

| This ADR rule | ADR-024 cross-reference |
|--------------|------------------------|
| repo-local API secrets remain forbidden | same supply-chain posture as `VEX_MODEL_TOKEN` and `[[mcp_servers]]` restrictions |
| MCP tools must use `mcp.<server>.<tool>` names | Gap 5 / PF-01 namespace contract |
| `vex remote-control` remains out of scope | Phase I exclusion / deferred-indefinitely boundary |
| local transport permissions and guards are additional containment rules | additive to ADR-024's sandbox/config safety posture |

| Rule | Enforcement |
|------|-------------|
| Do not make JSONL the canonical trait contract | `RuntimeEnvelope` / `RuntimeRequest` are the canonical layer; JSONL is a serialization |
| Do not expose provider-native stream chunks past the normalization layer | Required by PI-10 |
| Do not break existing `vex exec --format jsonl` output under this ADR | BatchMode compatibility rule |
| Every machine-readable runtime seam must map to `RuntimeEnvelope` or `RuntimeRequest` | Mandatory for new adapters |
| Every envelope must include `task_id`, `turn`, and `seq` | Required by schema and round-trip tests |
| `seq` must reset to 1 at the start of each turn | Required by schema and PI-12 test assertion |
| `Error.recoverable` must have defined semantics | Required by schema and adapter tests |
| MCP tools must use the `mcp.<server>.<tool>` namespace | Required by grammar and schema pattern validation |
| Do not add HTTP routes, auth, or SSE framing here | Explicitly out of scope for ADR-025 |
| Do not implement `vex remote-control` here | Requires separate ADR, still out of scope |
| `ToolCall.id` must be generated by the runtime normalization layer | Provider ids are discarded at the normalization boundary; no model backend may own this field |
| `tool_name` in `$defs.tool_result` must use `$ref` to `$defs.tool_name`, not an inlined pattern | Schema DRY rule; enforced in PI-11 |
| `MaxTurnsReached` must always be followed by `TurnEnd { status: "failed" }` | Emitted as a pair; enforced by PI-12 test |
| `TurnEnd.status` values are `"completed"`, `"failed"`, `"cancelled"` only | Schema enum; additions require a version bump |
| `ApproveCapability` and `DenyCapability` must be present in Appendix C schema | Required for schema/Rust-type parity |

---

## Appendix A: GBNF grammar (`grammars/tool_call.gbnf`)

The grammar below constrains local-model constrained-decoding output. It is intentionally conservative: the `json_safe_char` set covers common identifier and path characters but excludes some valid JSON string characters (e.g. `(`, `)`, `;`, `$`). Backends that do not use constrained decoding are not restricted by this grammar; PI-10 normalization and serde accept the full JSON character set from those backends.

```gbnf
root ::= ws tool_call_array ws

tool_call_array ::= "[" ws tool_call (ws "," ws tool_call)* ws "]" | tool_call

tool_call ::= "{" ws "\"name\"" ws ":" ws tool_name ws "," ws "\"arguments\"" ws ":" ws json_object ws "}"

# id is NOT included in the grammar — it is injected by the runtime normalization layer (PI-10).

tool_name ::= "\"read_file\""
            | "\"write_file\""
            | "\"apply_patch\""
            | "\"run_command\""
            | "\"search_files\""
            | "\"list_dir\""
            | "\"glob_files\""
            | mcp_tool

mcp_tool ::= "\"mcp." server_name "." tool_segment "\""

server_name ::= lower server_rest*
server_rest ::= lower | digit | "_" | "-"

tool_segment ::= lower tool_rest*
tool_rest ::= lower | digit | "_"

json_value ::= json_object
             | json_array
             | json_string
             | json_number
             | "true"
             | "false"
             | "null"

json_object ::= "{" ws "}"
              | "{" ws json_member (ws "," ws json_member)* ws "}"

json_member ::= json_string ws ":" ws json_value

json_array ::= "[" ws "]"
             | "[" ws json_value (ws "," ws json_value)* ws "]"

json_string ::= "\"" json_char* "\""
json_char ::= json_safe_char | escape

json_safe_char ::= lower | upper | digit | "_" | "-" | "." | "/" | ":" | " " | "[" | "]" | "{" | "}" | "," | "@" | "#" | "+" | "="

escape ::= "\\\"" | "\\\\" | "\\/" | "\\b" | "\\f" | "\\n" | "\\r" | "\\t"
         | "\\u" hex hex hex hex

json_number ::= "-"? int frac? exp?
int ::= "0" | onenine digits?
frac ::= "." digits
exp ::= ("e" | "E") ("+" | "-")? digits

digits ::= digit+
digit ::= "0" | onenine
onenine ::= "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"
# Lowercase-only hex is intentional for constrained decoding; see §5 note on json_safe_char.
# Standard JSON allows uppercase A-F in \uXXXX escapes, but the grammar restricts to
# lowercase to limit the local-model output surface. PI-10 normalization accepts both cases.
hex ::= digit | "a" | "b" | "c" | "d" | "e" | "f"

lower ::= "a" | "b" | "c" | "d" | "e" | "f" | "g" | "h" | "i" | "j" | "k" | "l" | "m" | "n" | "o" | "p" | "q" | "r" | "s" | "t" | "u" | "v" | "w" | "x" | "y" | "z"
upper ::= "A" | "B" | "C" | "D" | "E" | "F" | "G" | "H" | "I" | "J" | "K" | "L" | "M" | "N" | "O" | "P" | "Q" | "R" | "S" | "T" | "U" | "V" | "W" | "X" | "Y" | "Z"

ws ::= (" " | "\n" | "\r" | "\t")*
```

---

## Appendix B: JSON Schema (`schemas/runtime_envelope_v1.json`)

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://vexcoder.io/schemas/runtime_envelope_v1.json",
  "title": "RuntimeEnvelope v1",
  "type": "object",
  "additionalProperties": false,
  "required": ["version", "task_id", "turn", "seq", "event"],
  "properties": {
    "version": { "type": "integer", "const": 1 },
    "task_id": { "type": "string", "minLength": 1 },
    "turn": { "type": "integer", "minimum": 1 },
    "seq": { "type": "integer", "minimum": 1 },
    "event": { "$ref": "#/$defs/runtime_event" }
  },
  "$defs": {
    "tool_name": {
      "type": "string",
      "pattern": "^(read_file|write_file|apply_patch|run_command|search_files|list_dir|glob_files|mcp\\.[a-z][a-z0-9_-]*\\.[a-z][a-z0-9_]*)$"
    },
    "scope": {
      "type": "string",
      "enum": ["once", "session"]
    },
    "token_usage": {
      "type": "object",
      "additionalProperties": false,
      "required": ["input", "output"],
      "properties": {
        "input": { "type": "integer", "minimum": 0 },
        "output": { "type": "integer", "minimum": 0 },
        "estimated": { "type": "boolean", "default": false }
      }
    },
    "validation_output": {
      "type": "object",
      "additionalProperties": false,
      "required": ["label", "exit_code", "stdout_tail", "stderr_tail"],
      "properties": {
        "label": { "type": "string", "minLength": 1 },
        "exit_code": { "type": "integer" },
        "stdout_tail": { "type": "string" },
        "stderr_tail": { "type": "string" }
      }
    },
    "runtime_event": {
      "oneOf": [
        { "$ref": "#/$defs/turn_start" },
        { "$ref": "#/$defs/assistant_delta" },
        { "$ref": "#/$defs/assistant_message" },
        { "$ref": "#/$defs/tool_call" },
        { "$ref": "#/$defs/tool_result" },
        { "$ref": "#/$defs/approval_request" },
        { "$ref": "#/$defs/approval_resolved" },
        { "$ref": "#/$defs/validation_result" },
        { "$ref": "#/$defs/turn_end" },
        { "$ref": "#/$defs/error" },
        { "$ref": "#/$defs/max_turns_reached" }
      ]
    },
    "turn_start": {
      "type": "object",
      "additionalProperties": false,
      "required": ["type"],
      "properties": {
        "type": { "const": "turn_start" },
        "input": { "type": ["string", "null"] }
      }
    },
    "assistant_delta": {
      "type": "object",
      "additionalProperties": false,
      "required": ["type", "text"],
      "properties": {
        "type": { "const": "assistant_delta" },
        "text": { "type": "string" }
      }
    },
    "assistant_message": {
      "type": "object",
      "additionalProperties": false,
      "required": ["type", "content"],
      "properties": {
        "type": { "const": "assistant_message" },
        "content": { "type": "string" }
      }
    },
    "tool_call": {
      "type": "object",
      "additionalProperties": false,
      "required": ["type", "id", "name", "arguments"],
      "properties": {
        "type": { "const": "tool_call" },
        "id": { "type": "string", "pattern": "^call_[0-9]+_[0-9a-f]{4}$" },
        "name": { "$ref": "#/$defs/tool_name" },
        "arguments": { "type": "object" }
      }
    },
    "tool_result": {
      "type": "object",
      "additionalProperties": false,
      "required": ["type", "tool_call_id", "is_error", "output"],
      "properties": {
        "type": { "const": "tool_result" },
        "tool_call_id": { "type": "string", "pattern": "^call_[0-9]+_[0-9a-f]{4}$" },
        "tool_name": {
          "oneOf": [
            { "$ref": "#/$defs/tool_name" },
            { "type": "null" }
          ]
        },
        "is_error": { "type": "boolean" },
        "output": { "type": "string" }
      }
    },
    "approval_request": {
      "type": "object",
      "additionalProperties": false,
      "required": ["type", "capability", "scope"],
      "properties": {
        "type": { "const": "approval_request" },
        "capability": { "type": "string", "minLength": 1 },
        "scope": { "$ref": "#/$defs/scope" },
        "tool_name": { "type": ["string", "null"] }
      }
    },
    "approval_resolved": {
      "type": "object",
      "additionalProperties": false,
      "required": ["type", "capability", "scope", "approved"],
      "properties": {
        "type": { "const": "approval_resolved" },
        "capability": { "type": "string", "minLength": 1 },
        "scope": { "$ref": "#/$defs/scope" },
        "approved": { "type": "boolean" }
      }
    },
    "validation_result": {
      "type": "object",
      "additionalProperties": false,
      "required": ["type", "passed", "outputs"],
      "properties": {
        "type": { "const": "validation_result" },
        "passed": { "type": "boolean" },
        "outputs": {
          "type": "array",
          "items": { "$ref": "#/$defs/validation_output" }
        }
      }
    },
    "turn_end": {
      "type": "object",
      "additionalProperties": false,
      "required": ["type", "status", "changed_files"],
      "properties": {
        "type": { "const": "turn_end" },
        "status": { "type": "string", "enum": ["completed", "failed", "cancelled"] },
        "usage": {
          "oneOf": [
            { "$ref": "#/$defs/token_usage" },
            { "type": "null" }
          ]
        },
        "changed_files": {
          "type": "array",
          "items": { "type": "string", "minLength": 1 }
        }
      }
    },
    "error": {
      "type": "object",
      "additionalProperties": false,
      "required": ["type", "code", "message", "recoverable"],
      "properties": {
        "type": { "const": "error" },
        "code": { "type": "string", "minLength": 1 },
        "message": { "type": "string", "minLength": 1 },
        "recoverable": { "type": "boolean" }
      }
    },
    "max_turns_reached": {
      "type": "object",
      "additionalProperties": false,
      "required": ["type", "max_turns"],
      "properties": {
        "type": { "const": "max_turns_reached" },
        "max_turns": { "type": "integer", "minimum": 1 }
      }
    }
  }
}
```

---

## Appendix C: JSON Schema (`schemas/runtime_request_v1.json`)

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://vexcoder.io/schemas/runtime_request_v1.json",
  "title": "RuntimeRequest v1",
  "$defs": {
    "scope": {
      "type": "string",
      "enum": ["once", "session"]
    }
  },
  "oneOf": [
    {
      "type": "object",
      "additionalProperties": false,
      "required": ["type", "input"],
      "properties": {
        "type": { "const": "submit_input" },
        "task_id": { "type": ["string", "null"] },
        "input": { "type": "string", "minLength": 1 }
      }
    },
    {
      "type": "object",
      "additionalProperties": false,
      "required": ["type", "task_id"],
      "properties": {
        "type": { "const": "interrupt" },
        "task_id": { "type": "string", "minLength": 1 }
      }
    },
    {
      "type": "object",
      "additionalProperties": false,
      "required": ["type", "task_id", "capability", "scope"],
      "properties": {
        "type": { "const": "approve_capability" },
        "task_id": { "type": "string", "minLength": 1 },
        "capability": { "type": "string", "minLength": 1 },
        "scope": { "$ref": "#/$defs/scope" }
      }
    },
    {
      "type": "object",
      "additionalProperties": false,
      "required": ["type", "task_id", "capability"],
      "properties": {
        "type": { "const": "deny_capability" },
        "task_id": { "type": "string", "minLength": 1 },
        "capability": { "type": "string", "minLength": 1 }
      }
    }
  ]
}
```

---

## References

- `adr/ADR-024-zero-licensing-cost-agent-parity-gaps.md` — Phase I reservation and BatchMode compatibility surface
- `adr/ADR-023-deterministic-edit-loop.md` — validation and edit-loop event sources
- `adr/completed/ADR-006-runtime-mode-contracts.md` — runtime seam types and responsibilities
- `src/types/api_types.rs` — existing provider-facing message and tool-result shapes
- `src/batch_mode.rs` — existing monolithic `TurnRecord` / `SummaryRecord` JSONL output
