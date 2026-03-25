# ADR-029: Stream Parser Completeness and Session Persistence Extensions

**Date:** 2026-03-15
**Status:** Accepted — verification suite completed 2026-03-25
**Deciders:** Core maintainer
**ADR chain:** ADR-020, ADR-022, ADR-023, ADR-025

## Context

The SSE stream parser in `src/api/stream.rs` and the type surface in
`src/types/api_types.rs` currently recognise seven `StreamEvent` variants:
`MessageStart`, `ContentBlockStart`, `ContentBlockDelta`, `ContentBlockStop`,
`MessageDelta`, `MessageStop`, and a catch-all `Unknown`.

Two gaps limit orchestrator-level use of this surface.

First, several event and delta types defined in the streaming protocol are not
represented as distinct variants. Server-sent error events arrive as structured
JSON with a typed error envelope but are silently absorbed by `Unknown` and
never surfaced to the caller. Extended-thinking output streams produce
`thinking_delta` and `signature_delta` delta types whose payload fields are not
present in `Delta`. Thinking and redacted-thinking content blocks are legal
`ContentBlockStart` payloads but are not parsed. Cache usage fields
(`cache_creation_input_tokens`, `cache_read_input_tokens`) appear in
`MessageStart` and `MessageDelta` usage objects but are dropped. The stop
sequence string is absent from `MessageDelta`. These omissions mean the runtime
loop cannot react to error conditions, cannot surface thinking output, and
cannot track cache token budgets.

Second, `TaskState` records the evidence trail of a task but does not persist
three categories of session data that an orchestrator needs when resuming an
interrupted run: the active plan produced by the `/plan` command, session notes
written by the `/memory` command, and conversation-window compaction events
that record how the rolling context was trimmed. Without these, a resumed task
has evidence but lacks the planning context and memory anchors that guided the
prior session.

## Decision

### 1. Expand `StreamEvent`

Add two new variants to the `StreamEvent` enum:

```rust
Ping,
Error { error: ApiStreamError },
```

`Ping` represents the server keep-alive heartbeat and carries no data. The
new variant must be produced when the parser sees `event: ping` so the
messages v1 heartbeat is represented explicitly instead of being dropped.

`Error` carries a structured error envelope. Any frame whose JSON deserialises
to `{"type":"error",...}` must be surfaced as this variant rather than absorbed
by `Unknown`. The runtime loop is responsible for deciding whether a stream
error is retryable; the parser's job is only to represent it faithfully.

### 2. Expand `ContentBlock`

Add four new variants to the `ContentBlock` enum:

```rust
Thinking { thinking: String, signature: String },
RedactedThinking { data: String },
ServerToolUse { id: String, name: String, input: serde_json::Value },
WebSearchToolResult { tool_use_id: String, content: serde_json::Value },
```

Additionally, the existing `Text` variant gains an optional `citations` field:

```rust
Text { text: String, citations: Option<Vec<serde_json::Value>> },
```

`Thinking` and `RedactedThinking` cover extended-thinking model sessions.
`ServerToolUse` represents server-side tool invocations (e.g., web search)
that the API executes internally. `WebSearchToolResult` carries the results
of such invocations. `citations` on `Text` captures citation metadata that
the API may attach to text blocks when citations are enabled.

Additionally, the existing `ToolUse` variant gains optional parser metadata so
chat-completions tool-call chunks no longer lose their type discriminator or
choice index during normalization:

```rust
ToolUse {
    id: String,
    name: String,
    input: serde_json::Value,
    metadata: Option<ToolUseMetadata>,
}

pub struct ToolUseMetadata {
    pub call_type: Option<String>,
    pub choice_index: Option<usize>,
}
```

Callers that do not use these features may ignore the variants; the runtime
is not required to display them in the TUI by this ADR.

### 3. Expand `Delta`

Add three optional fields to the existing `Delta` struct:

```rust
pub thinking: Option<String>,
pub signature: Option<String>,
pub choice_index: Option<usize>,
```

These carry the incremental content for `thinking_delta` and `signature_delta`
delta types. `choice_index` preserves the originating chat-completions choice
number when the parser normalizes a `choices[]` chunk into unified text or
tool-argument delta events. The `delta_type` field already records the type
tag; the new fields provide the matching payload slots and the source-choice
anchor.

### 4. Expand `ApiUsage`

Add protocol-normalised token fields, cache-usage fields, extended usage
metadata, and detailed token breakdowns:

```rust
pub struct ApiUsage {
    // Core token counts (cross-protocol normalised)
    pub input_tokens: Option<u64>,     // alias: prompt_tokens
    pub output_tokens: Option<u64>,    // alias: completion_tokens
    pub total_tokens: Option<u64>,
    // Provider cache fields
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation: Option<serde_json::Value>,
    // Extended usage metadata
    pub service_tier: Option<String>,
    pub web_search_requests: Option<u64>,
    pub inference_geo: Option<String>,
    // Detailed token breakdowns (chat completions)
    pub prompt_tokens_details: Option<PromptTokenDetails>,
    pub completion_tokens_details: Option<CompletionTokenDetails>,
}

pub struct PromptTokenDetails {
    pub cached_tokens: Option<u64>,
    pub audio_tokens: Option<u64>,
}

pub struct CompletionTokenDetails {
    pub reasoning_tokens: Option<u64>,
    pub audio_tokens: Option<u64>,
    pub accepted_prediction_tokens: Option<u64>,
    pub rejected_prediction_tokens: Option<u64>,
}
```

`input_tokens` and `output_tokens` are the normalised in-memory field names.
For messages v1 they deserialise from the same-named fields. For chat
completions they deserialise from `prompt_tokens` and `completion_tokens`
via serde aliases. `total_tokens` captures the chat-completions total.

`cache_creation` captures the messages v1 cache-creation breakdown object
(ephemeral durations). `service_tier` and `web_search_requests` are
extended-usage metadata from the messages v1 protocol. `inference_geo`
preserves the documented model-execution geography hint. `prompt_tokens_details`
and `completion_tokens_details` carry the chat completions token-detail
breakdowns (cached tokens, reasoning tokens, audio tokens, prediction tokens).

All fields are optional and default to `None` when absent.

### 5. Expand `MessageDelta` and fix `message_delta` usage location

Add the stop sequence string to `MessageDelta`:

```rust
pub stop_sequence: Option<String>,
```

This field is present in the protocol when `stop_reason` is `"stop_sequence"`
and carries the exact sequence that terminated the generation.

**Critical wire-format fix:** The messages v1 `message_delta` event carries
`usage` as a **top-level peer** of `delta`, not nested inside the `delta`
object. The `StreamEvent::MessageDelta` variant must therefore carry `usage`
at the variant level:

```rust
MessageDelta {
    delta: MessageDelta,
    usage: Option<ApiUsage>,
},
```

The `MessageDelta` struct itself contains only `stop_reason` and
`stop_sequence`. This matches the documented wire format:
`{"type":"message_delta","delta":{...},"usage":{...}}`.

### 5a. Expand `MessageStartData`

The documented `message_start` payload includes a full `Message` object.
`MessageStartData` is expanded to capture all documented fields, plus optional
normalized stream metadata used by chat-compat chunks:

```rust
pub struct MessageStartData {
    pub id: String,
    pub message_type: Option<String>,  // serde rename from "type"
    pub role: String,
    pub model: String,
    pub content: Option<Vec<serde_json::Value>>,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: Option<ApiUsage>,
    pub metadata: Option<StreamChunkMetadata>,
}
```

### 5b. Expand chat-completions streaming surface

The internal chat-compat deserialization types are expanded to capture the
full documented chat completions streaming chunk surface:

- `ChatCompatChunk`: `id`, `object`, `created`, `model`,
  `system_fingerprint`, `service_tier`, `choices`, `usage`
- `ChatCompatChoice`: `index`, `delta`, `finish_reason`, `logprobs`
- `ChatCompatDelta`: `role`, `content`, `refusal`, `tool_calls`
- `ChatCompatToolCallDelta`: `index`, `id`, `type`, `function`

The normalization layer must also retain those values when it emits unified
events:

- top-level chunk metadata (`object`, `created`, `system_fingerprint`,
  `service_tier`) is preserved in `MessageStartData.metadata` on the synthetic
  start event and in `MessageDelta.metadata` when emitted later in the stream;
- `choices[].index` is preserved on `Delta.choice_index`,
  `ToolUseMetadata.choice_index`, and `MessageDelta.metadata.choice_index`;
- `choices[].delta.role` seeds the synthetic `MessageStart` role and is
  retained on `MessageDelta.role` when a later message-delta event is emitted;
- `choices[].delta.refusal` is preserved on `MessageDelta.refusal`;
- `choices[].logprobs` is preserved on `MessageDelta.metadata.logprobs`;
- `tool_calls[].type` is preserved on `ToolUseMetadata.call_type`;
- final usage-only chunks keep `usage` even when `choices` is empty, and the
  top-level `service_tier` must be copied into `ApiUsage.service_tier` when the
  usage payload omits it.

Supporting metadata struct:

```rust
pub struct StreamChunkMetadata {
    pub object: Option<String>,
    pub created: Option<u64>,
    pub system_fingerprint: Option<String>,
    pub service_tier: Option<String>,
    pub choice_index: Option<usize>,
    pub logprobs: Option<serde_json::Value>,
}
```

### 6. Add `ApiStreamError`

Add a new struct for the stream error envelope:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ApiStreamError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}
```

### 7. Extend `TaskState` with plan, session notes, compaction records, and cache stats

Add four new optional fields to `TaskState`, all gated with `#[serde(default)]`
to preserve backward compatibility with existing state files:

```rust
#[serde(default)]
pub plan: Option<String>,

#[serde(default)]
pub session_notes: Vec<SessionNote>,

#[serde(default)]
pub context_compaction: Vec<ContextCompactionRecord>,

#[serde(default)]
pub cache_usage: CacheUsageStats,
```

Supporting types:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionNote {
    pub content: String,
    pub created_at_turn: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextCompactionRecord {
    pub turn_index: usize,
    pub messages_before: usize,
    pub messages_after: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheUsageStats {
    pub total_cache_creation_tokens: u64,
    pub total_cache_read_tokens: u64,
}
```

**`plan`** holds the most recently written plan text. The `/plan` command
already produces a plan string; this field persists it across process
boundaries so a resumed task can recover the planning context without
re-running the plan assembly.

**`session_notes`** holds entries written by the `/memory` command or by the
runtime's note-injection path (ADR-024 Gap 16,
`TASKS/PJ-03-memory-notes-injection.md`).
Each note records the turn index at which it was created so a resumed session
can present notes in context order.

**`context_compaction`** records each event where the rolling conversation
window was trimmed. Each record captures the message count before and after
trimming and the summary text injected in place of the dropped messages. This
allows a resumed task to reconstruct the compaction history and avoid
re-injecting stale summaries.

**`cache_usage`** accumulates cache token metrics across turns. The per-turn
token evidence already records input and output tokens; this field adds
cumulative cache metrics so the operator can inspect total cache savings over
a multi-turn task without summing per-turn records.

### 8. Disk save contract for resume

The existing atomic-write path in `TaskState::save()` is unchanged. The new
fields are serialised as part of the same JSON document and written atomically
with the existing fields. No separate file paths are introduced.

Fields with `#[serde(default)]` deserialise to their default values when
reading state files written before this ADR, preserving backward compatibility.

State files written after this ADR include all four new fields.

The `--resume` flag path in `src/bin/vex.rs` is unchanged. It calls
`TaskState::load()` which reads the JSON document and populates all fields
including the new ones. The resumed session has access to the plan, notes,
compaction history, and cache stats immediately after load.

## Normative Type Surface

### `StreamEvent` (complete, post-ADR-029)

```rust
pub enum StreamEvent {
    MessageStart { message: MessageStartData },
    ContentBlockStart { index: usize, content_block: ContentBlock },
    ContentBlockDelta { index: usize, delta: Delta },
    ContentBlockStop { index: usize },
    MessageDelta { delta: MessageDelta, usage: Option<ApiUsage> },
    MessageStop,
    Ping,
    Error { error: ApiStreamError },
    #[serde(other)]
    Unknown,
}
```

### `ContentBlock` (complete, post-ADR-029)

```rust
pub enum ContentBlock {
    Text { text: String, citations: Option<Vec<serde_json::Value>> },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
    Thinking { thinking: String, signature: String },
    RedactedThinking { data: String },
    ServerToolUse { id: String, name: String, input: serde_json::Value },
    WebSearchToolResult { tool_use_id: String, content: serde_json::Value },
}
```

### `Delta` (complete, post-ADR-029)

```rust
pub struct Delta {
    pub delta_type: Option<String>,
    pub text: Option<String>,
    pub partial_json: Option<String>,
    pub thinking: Option<String>,
    pub signature: Option<String>,
}
```

### `MessageStartData` (complete, post-ADR-029)

```rust
pub struct MessageStartData {
    pub id: String,
    pub message_type: Option<String>,
    pub role: String,
    pub model: String,
    pub content: Option<Vec<serde_json::Value>>,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: Option<ApiUsage>,
}
```

### `ApiUsage` (normalised across messages v1 and chat completions, post-ADR-029)

```rust
pub struct ApiUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation: Option<serde_json::Value>,
    pub service_tier: Option<String>,
    pub web_search_requests: Option<u64>,
    pub prompt_tokens_details: Option<PromptTokenDetails>,
    pub completion_tokens_details: Option<CompletionTokenDetails>,
}

pub struct PromptTokenDetails {
    pub cached_tokens: Option<u64>,
    pub audio_tokens: Option<u64>,
}

pub struct CompletionTokenDetails {
    pub reasoning_tokens: Option<u64>,
    pub audio_tokens: Option<u64>,
    pub accepted_prediction_tokens: Option<u64>,
    pub rejected_prediction_tokens: Option<u64>,
}
```

### `TaskState` (additive fields, post-ADR-029)

```rust
pub struct TaskState {
    // existing fields unchanged
    pub id: TaskId,
    pub status: TaskStatus,
    pub active_grants: HashMap<Capability, ApprovalScope>,
    pub changed_files: Vec<PathBuf>,
    pub command_history: Vec<CommandEvidence>,
    pub conversation_snapshot: ConversationCheckpoint,
    pub interrupted_sessions: Vec<InterruptedCommand>,
    pub branch_name: Option<String>,
    pub instructions_path: Option<String>,
    pub turns: Vec<TurnEvidenceState>,
    // new fields
    pub plan: Option<String>,
    pub session_notes: Vec<SessionNote>,
    pub context_compaction: Vec<ContextCompactionRecord>,
    pub cache_usage: CacheUsageStats,
}
```

## Validation and Acceptance

### Required tests

**Original ADR-029 tests (all passing):**
- Deserialising a `StreamEvent` from `{"type":"error","error":{"type":"overloaded_error","message":"..."}}` produces `StreamEvent::Error` with the correct fields.
- Deserialising a ping frame through `StreamParser::process()` produces `StreamEvent::Ping`.
- Deserialising a `ContentBlockStart` with `{"type":"thinking","thinking":"...","signature":"..."}` produces `ContentBlock::Thinking`.
- A `Delta` with `{"type":"thinking_delta","thinking":"..."}` deserialises with the `thinking` field populated.
- An `ApiUsage` with `{"cache_creation_input_tokens":100}` deserialises with that field present and others absent.
- A chat-completions usage chunk with `{"prompt_tokens":...,"completion_tokens":...,"total_tokens":...}` is surfaced as `MessageDelta { usage: ... }` with the normalised token fields populated.
- A `TaskState` written with the new fields round-trips through `save()` and `load()` with all four new fields intact.
- A `TaskState` written before ADR-029 (without the new fields) loads without error; new fields default to empty/None.
- Accumulating cache tokens across three simulated turns produces correct totals in `CacheUsageStats`.

**Stream surface coverage tests (added for full protocol coverage):**
- A messages v1 `message_delta` event with **top-level** usage (`{"type":"message_delta","delta":{...},"usage":{...}}`) deserialises with usage at the event level, not inside the delta.
- A full `message_start` event deserialises `MessageStartData` with `message_type`, `content`, `stop_reason`, `stop_sequence`, and `usage`.
- A `ContentBlock` text block with citations deserialises the `citations` array.
- A `ContentBlock` with `"type":"server_tool_use"` deserialises as `ServerToolUse`.
- A `ContentBlock` with `"type":"web_search_tool_result"` deserialises as `WebSearchToolResult`.
- An `ApiUsage` with extended fields (`service_tier`, `web_search_requests`, `cache_creation`) deserialises all fields.
- An `ApiUsage` with chat completions detail breakdowns (`prompt_tokens_details`, `completion_tokens_details`) deserialises the nested detail structs.
- A messages v1 `message_delta` SSE frame through `StreamParser::process()` surfaces usage at the event level.

### Completion condition

All seventeen required tests pass under `cargo test --all-targets`. The new
event variants are reachable from match arms in the runtime loop without a
compiler warning about unreachable patterns. Existing tests are unaffected.

## Verification status

As of 2026-03-25, the repository proves all seventeen required test points with
named tests in the current tree:

**Original ADR-029 tests (9 points):**

1. `StreamEvent::Error` deserialisation:
   `src/types/api_types.rs::test_stream_event_error_deserialises`
2. Ping frame through `StreamParser::process()`:
   `src/api/stream.rs::test_process_emits_ping_for_ping_frame`
3. `ContentBlock::Thinking` deserialisation:
   `src/types/api_types.rs::test_content_block_thinking_deserialises`
4. `Delta` thinking fields:
   `src/types/api_types.rs::test_delta_thinking_fields_deserialise`
5. `ApiUsage` cache fields:
   `src/types/api_types.rs::test_api_usage_cache_fields_deserialise`
6. Chat-completions usage normalisation:
   `src/api/stream.rs::test_process_maps_chat_compat_usage_chunk`
7. `TaskState` round-trip with new fields:
   `src/runtime/task_state.rs::test_task_state_survives_atomic_write_and_reload`
8. `TaskState` backward compatibility (pre-ADR-029 files):
   `src/runtime/task_state.rs::test_task_state_pre_adr029_file_loads_with_default_new_fields`
9. `CacheUsageStats` accumulation:
   `src/runtime/task_state.rs::test_cache_usage_stats_accumulate`

**Stream surface coverage tests (8 points):**

10. `message_delta` top-level usage wire format:
    `src/types/api_types.rs::test_message_delta_event_top_level_usage_deserialises`
11. Full `message_start` deserialisation:
    `src/types/api_types.rs::test_message_start_full_message_deserialises`
12. `ContentBlock::Text` with citations:
    `src/types/api_types.rs::test_content_block_text_with_citations_deserialises`
13. `ContentBlock::ServerToolUse`:
    `src/types/api_types.rs::test_content_block_server_tool_use_deserialises`
14. `ContentBlock::WebSearchToolResult`:
    `src/types/api_types.rs::test_content_block_web_search_tool_result_deserialises`
15. `ApiUsage` extended fields:
    `src/types/api_types.rs::test_api_usage_extended_fields_deserialise`
16. `ApiUsage` chat-completions detail breakdowns:
    `src/types/api_types.rs::test_api_usage_chat_compat_detail_breakdowns_deserialise`
17. `message_delta` SSE frame through `StreamParser::process()`:
    `src/api/stream.rs::test_process_messages_v1_message_delta_top_level_usage`

### Multi-agent orchestration dependency

ADR-029 is a declared dependency of ADR-030, making it a prerequisite for full
invariant compliance. Specifically:

- `StreamEvent::Error` as a typed variant lets an orchestrating agent detect and
  react to sub-agent stream failures rather than silently absorbing them.
- The `TaskState` extensions (`plan`, `session_notes`, `context_compaction`,
  `cache_usage`) are exactly the handoff payload that lets an orchestrator
  reconstruct a sub-agent's context on resume. Without these, multi-agent task
  handoffs are lossy.
- `CacheUsageStats` maps to token-budget awareness across turns, closing an
  OpenCode parity gap.
- `ContentBlock::Thinking` support closes the extended-thinking parity gap.

### Runtime wiring (implemented 2026-03-25)

Three runtime data paths were connected to close the gap between the type
surface (already defined) and actual use at runtime:

1. **Cache usage accumulation** — `accumulate_usage()` in `core.rs` now
   extracts `cache_creation_input_tokens` and `cache_read_input_tokens` from
   `ApiUsage` into `TurnTokens`. `SessionTokens` tracks per-turn and
   cumulative cache fields. `commit_completed_turn()` in `turn.rs` copies
   the last turn's cache values into `TaskState.cache_usage`.

2. **Context compaction recording** — `compact_for_context_overflow()` in
   `core.rs` emits a `ConversationStreamUpdate::ContextCompacted` event.
   The event flows through `forward_conversation_update()` in `context.rs`
   as `UiUpdate::ContextCompacted`, and is recorded in
   `TaskState.context_compaction` by `model_update.rs`.

3. **Plan persistence** — The `/plan` command sets `plan_turn_active` on the
   TUI mode. When `commit_completed_turn()` runs, the turn's response text
   is written to `TaskState.plan`.

## Consequences

### Benefits

- Stream errors surfaced as typed variants allow the orchestrator loop to
  react (log, retry, surface to operator) rather than silently discard.
- Extended thinking output is representable end-to-end without dropping data.
- Cache usage accumulates per task, enabling token-budget awareness across
  multi-turn sessions.
- Plan, notes, and compaction history survive process interruption and are
  available immediately on resume without additional assembly steps.
- All new `TaskState` fields are backward-compatible with existing state files.

### Tradeoffs

- The `ContentBlock` and `StreamEvent` enums grow; downstream match arms that
  do not yet handle thinking blocks will produce compiler warnings until
  updated.
- `CacheUsageStats` accumulation requires the turn-completion path to extract
  cache fields from per-turn `ApiUsage` and add them to the task total; this
  wires a new data path through the existing save point.

## Compliance Notes for Agents

- Do not add provider-branded field names to the type surface.
- `Ping` must remain a no-data variant; do not add payload fields to it.
- Do not merge `ApprovalPolicy` and `RuntimeCorePolicy` concerns.
- Match arms handling `StreamEvent` must cover `Ping` and `Error` explicitly;
  relying on the `Unknown` catch-all for these variants is not compliant.
- New `TaskState` fields must use `#[serde(default)]` to preserve backward
  compatibility with pre-ADR-029 state files.
