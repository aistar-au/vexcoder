# ADR-029: Stream Parser Completeness and Session Persistence

**Status:** Accepted (all 8 decision items verified 2026-03-28)  
**Chain:** ADR-020, ADR-022, ADR-023, ADR-025

## Context

`StreamEvent`, `ContentBlock`, `Delta`, and `ApiUsage` lacked variants required for full messages/v1 and chat-compatible response coverage. `TaskState` had no fields for plan, notes, or compaction metadata.

## Decision

- `StreamEvent` gains `Ping` and `Error` variants.
- `ContentBlock` gains `Thinking`, `ThinkingData`, `ServerToolUse`, `WebSearchToolResult`; `Text` variant gains optional `citations` field.
- `Delta` gains `thinking`, `signature`, `choice_index` fields.
- `ApiUsage` unifies across protocols: cache, service tier, and per-token-type detail fields.
- `MessageDelta` carries top-level `usage` and `stop_sequence`.
- `TaskState` gains `plan`, `session_notes`, `context_compaction`, `cache_usage` fields with `#[serde(default)]` for backward compatibility.
- `StreamTextNormaliser` auto-closes stale tool blocks and normalizes embedded markup on content-block boundaries.
- All new `serde` fields use `#[serde(default)]` for pre-ADR-029 file compatibility.

## References

- [`serde`](https://docs.rs/serde) / [`serde_json`](https://docs.rs/serde_json) — event deserialization
- Server-Sent Events: <https://html.spec.whatwg.org/multipage/server-sent-events.html>
