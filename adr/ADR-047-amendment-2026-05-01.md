# ADR-047 Amendment (2026-05-01): Tagged XML Tool-Call Normalization at Ingress and Structured-Round Protocol Selection

**Status:** Amended  
**Amends:** ADR-047, ADR-047-amendment-2026-04-20  
**PR:** #429 (`work/vexcoder-remove-tagged-xml-fallback`)

## Amendment

### 1. Tagged XML tool calls normalized entirely at ingress (PR #429)

Before PR #429, the runtime contained a dual-path seam in `send_message.rs`: if the model emitted XML-encoded tool calls (`<function=name><parameter=key>value</parameter></function>`), they were parsed in the message-send loop rather than at the ingress boundary. This violated the invariant established by ADR-047-amendment-2026-04-20 that "provider compatibility grammars are confined to immediate ingress."

PR #429 removes this seam entirely:

- `StreamTextNormaliser` now handles Hermes/Qwen3-Instruct JSON format (`<tool_call>{"name":"X","arguments":{...}}</tool_call>`) in addition to parameter-tagged XML and Qwen3-Coder outer-wrapper format.
- `protocol_ingress.rs` synthesizes native `ToolCallStarted` signals from all three tagged-tool-call surface formats, emitting them with source IDs prefixed `toulu_tagged_` to distinguish synthesized blocks from model-native `tool_calls` entries.
- `has_native_tool_calls` flag in `ProtocolIngressState` suppresses XML synthesis when the same turn already delivered a native `tool_calls` block, preventing duplicate tool call cards when a model echoes its call in both channels.
- `send_message.rs` `StreamBlock::ToolCall` handler classifies blocks by source ID prefix: IDs not starting with `toulu_tagged_` set `saw_native_tool_call_block = true`.
- All downstream consumers — TUI, batch, export — receive only `RuntimeEnvelope` events; no consumer branch inspects raw tagged XML. The `scrub_materialized_tool_markup` methods in `TuiMode` and batch adapter are now empty stubs, retained only for call-site compatibility.

### 2. Structured-round protocol selection for XML-only local models (commit 9fd4c8f)

PR #429 initially changed `use_structured_round` from:

```
use_structured_tool_protocol && !used_tagged_fallback
```

to:

```
use_structured_tool_protocol
```

This caused a regression for local models that produce only XML-encoded tool calls (e.g., models served at `0.0.0.0:8000` through a local inference runner without a chat template that emits native `tool_calls`). When the model generated XML tool calls and the runtime sent back `ContentBlock::ToolUse`/`ContentBlock::ToolResult` blocks (structured content-block format), the local model could not interpret the context and would loop.

Fix (commit 9fd4c8f): `use_structured_round` is now:

```rust
let use_structured_round = use_structured_tool_protocol
    && (!self.client.is_local_endpoint() || saw_native_tool_call_block);
```

`saw_native_tool_call_block` is set to `true` within a turn when any `StreamBlock::ToolCall` is opened with an ID that does not start with `toulu_tagged_`. This correctly distinguishes:

- **Local XML-only model**: all tool blocks have `toulu_tagged_` IDs → `saw_native_tool_call_block` stays `false` → `use_structured_round = false` → tool results enter history as plain text protocol, which the model understands.
- **Local model with native tool_calls support**: at least one block has a model-generated ID → `saw_native_tool_call_block = true` → `use_structured_round = true` → tool results enter history as `ContentBlock::ToolResult` (structured content-block format).
- **Remote endpoint**: `is_local_endpoint()` returns `false` → structured round always used regardless of tool call source.

This per-turn detection is persistent for the life of the turn. The flag is reset to `false` at the start of each tool round via the per-round variable declaration.
