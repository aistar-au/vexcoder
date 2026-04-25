# ADR-032: Prompt Area Interactivity and Context Guard

**Status:** Accepted (bottom-anchored prompt amendment corrected 2026-04-09)  
**Chain:** ADR-031, ADR-015

## Context

The prompt area lacked character-count feedback, file/command pickers, and automatic compaction on context overflow. Item 9 (hybrid retrieval) was transferred to ADR-033.

## Decision

- Character count indicator rendered in prompt status line.
- Focus indicator distinguishes focused input from unfocused scroll.
- `/compact` resets conversation history, preserving the active task.
- Automatic compaction on HTTP 400 context-overflow: retain last 4 messages, retry once.
- `@` file picker and `/` slash picker with arrow-key navigation.
- Context-proportional auto-cap for `read_file`: allocates ~10% of context budget per file.
- Bottom-anchored prompt rendered over the app-owned transcript surface (ADR-031).
- Items 10–14 align prompt/navigation with the owned-transcript review model; no host-scrollback dependency.

## References

- [`ratatui`](https://docs.rs/ratatui) — prompt widget
- [RFC 7231 §6.5.1](https://www.rfc-editor.org/rfc/rfc7231#section-6.5.1) — HTTP 400 Bad Request
