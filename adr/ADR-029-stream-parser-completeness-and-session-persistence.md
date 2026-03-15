# ADR-029: Stream Parser Completeness and Session Persistence Extensions

**Date:** 2026-03-15
**Status:** Proposed
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
existing parser already discards frames with `event: ping`; the new variant
makes the discard explicit and typed.

`Error` carries a structured error envelope. Any frame whose JSON deserialises
to `{"type":"error",...}` must be surfaced as this variant rather than absorbed
by `Unknown`. The runtime loop is responsible for deciding whether a stream
error is retryable; the parser's job is only to represent it faithfully.

### 2. Expand `ContentBlock`

Add two new variants to the `ContentBlock` enum:

```rust
Thinking { thinking: String, signature: String },
RedactedThinking { data: String },
```

These cover the two content block types emitted during extended-thinking model
sessions. Both are legal `content_block_start` payloads and must be
deserialised when present. Callers that do not use extended thinking may ignore
them; the runtime is not required to display thinking content in the TUI by
this ADR.

### 3. Expand `Delta`

Add two optional fields to the existing `Delta` struct:

```rust
pub thinking: Option<String>,
pub signature: Option<String>,
```

These carry the incremental content for `thinking_delta` and `signature_delta`
delta types. The `delta_type` field already records the type tag; the new
fields provide the matching payload slots.

### 4. Expand `ApiUsage`

Add three optional cache-usage fields to `ApiUsage`:

```rust
pub cache_creation_input_tokens: Option<u64>,
pub cache_read_input_tokens: Option<u64>,
pub cache_write_input_tokens: Option<u64>,
```

These are populated from the corresponding fields in `MessageStart` usage and
`MessageDelta` usage objects when the backend returns them. They are optional
because not all backends or all requests emit cache metrics.

### 5. Expand `MessageDelta`

Add the stop sequence string to `MessageDelta`:

```rust
pub stop_sequence: Option<String>,
```

This field is present in the protocol when `stop_reason` is `"stop_sequence"`
and carries the exact sequence that terminated the generation. It allows the
orchestrator loop to distinguish which stop sequence fired without parsing the
model output.

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
    pub total_cache_write_tokens: u64,
}
```

**`plan`** holds the most recently written plan text. The `/plan` command
already produces a plan string; this field persists it across process
boundaries so a resumed task can recover the planning context without
re-running the plan assembly.

**`session_notes`** holds entries written by the `/memory` command or by the
runtime's note-injection path (ADR-024 Gap 16, `TASKS/PJ-03-memory-notes.md`).
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
    MessageDelta { delta: MessageDelta },
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
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
    Thinking { thinking: String, signature: String },
    RedactedThinking { data: String },
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

### `ApiUsage` (complete, post-ADR-029)

```rust
pub struct ApiUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_write_input_tokens: Option<u64>,
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

- Deserialising a `StreamEvent` from `{"type":"error","error":{"type":"overloaded_error","message":"..."}}` produces `StreamEvent::Error` with the correct fields.
- Deserialising a `StreamEvent` from `[DONE]` or a ping frame does not produce `StreamEvent::Error` or `StreamEvent::Unknown`.
- Deserialising a `ContentBlockStart` with `{"type":"thinking","thinking":"...","signature":"..."}` produces `ContentBlock::Thinking`.
- A `Delta` with `{"type":"thinking_delta","thinking":"..."}` deserialises with the `thinking` field populated.
- An `ApiUsage` with `{"cache_creation_input_tokens":100}` deserialises with that field present and others absent.
- A `TaskState` written with the new fields round-trips through `save()` and `load()` with all four new fields intact.
- A `TaskState` written before ADR-029 (without the new fields) loads without error; new fields default to empty/None.
- Accumulating cache tokens across three simulated turns produces correct totals in `CacheUsageStats`.

### Completion condition

All eight required tests pass under `cargo test --all-targets`. The new event
variants are reachable from match arms in the runtime loop without a
compiler warning about unreachable patterns. Existing tests are unaffected.

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
