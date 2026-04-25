# ADR-041: Transcript Renderer Wiring and Compact Tool Paragraphs

**Status:** Accepted (2026-04-08 host-scrollback amendment deprecated 2026-04-09)  
**Chain:** ADR-029, ADR-030, ADR-031, ADR-040

## Context

Tool output was rendered inline as raw text, producing transcript paragraphs that grew unbounded. The renderer had no delta-native streaming path for incremental updates.

## Decision

- `StreamTextNormaliser` auto-closes stale tool blocks on new function-open tag; flushes on non-text boundary.
- Compact tool paragraph layout: pending shows header + Input only; completed shows header, Input, Result, max 3 evidence lines.
- Overflow escalates to detail surfaces (overlays, pagers); no inline expansion beyond 3 lines.
- `↑` / `↓` arrow labels replace `read:` / `generate:` string labels.
- Delta-native streaming block buffers with bounded suffix deduplication.
- Live input preview via incremental JSON parsing on `ToolCall` delta events.
- Word-wrap prose at display boundary; preserve structural markers (headings, fences, lists).
- Deprecated design context (D17–D22, host-scrollback path) retained as non-active reference only.

## References

- [`ratatui`](https://docs.rs/ratatui) — paragraph widget
- [`serde_json`](https://docs.rs/serde_json) — incremental ToolCall delta parsing
