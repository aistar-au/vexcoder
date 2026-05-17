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
- Transcript assembly builds `Vec<Line<'static>>` and wraps via `Text::from(lines)` at the
	flush boundary. The mutable builder pattern (`Text::push_line`, `Text::extend`) is not
	currently used; migration is tracked as a target in `TASKS/PR-400-ratatui-api-surface-map.md`.
- Deprecated design context (D17–D22, host-scrollback path) retained as non-active reference only.

## References

- [`Text::from(Vec<Line>)`](https://docs.rs/ratatui/0.30.0/ratatui/text/struct.Text.html) — transcript flush pattern; `Vec<Line<'static>>` assembled then wrapped (`src/ui/render/transcript.rs`)
- [`Line::from(Vec<Span>)`](https://docs.rs/ratatui/0.30.0/ratatui/text/struct.Line.html) — per-row span composition for ANSI-converted and styled transcript rows
- [`Span::styled(str, Style)`](https://docs.rs/ratatui/0.30.0/ratatui/text/struct.Span.html) — span styling throughout transcript renderer
- [`Paragraph::new(text).scroll((row, 0))`](https://docs.rs/ratatui/0.30.0/ratatui/widgets/struct.Paragraph.html) — transcript display with manual scroll offset (`src/ui/render/mod.rs:153`)
- [`ansi_to_tui::IntoText`](https://docs.rs/ansi-to-tui) — ANSI escape sequence to ratatui `Text` conversion (`src/ui/render/transcript.rs`)
- [`Style::default().add_modifier(Modifier::X)`](https://docs.rs/ratatui/0.30.0/ratatui/style/struct.Style.html) — inline styling for tool output, diff lines, and evidence rows
- [`serde_json`](https://docs.rs/serde_json) — incremental ToolCall delta parsing
