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
- Transcript row assembly builds `Line<'static>` values for the frame
  renderer. Primary task-output rows are wrapped with `Text::from_iter(...)`;
  fallback message rows use `Text::default()`, `push_line(...)`, and
  `height()` before `Paragraph::scroll(...)`. The remaining text-builder gaps
  are tracked in `TASKS/PR-400-ratatui-api-surface-map.md`.
- Deprecated design context (D17-D22, host-scrollback path) retained as non-active reference only.

## References

- [`Text::from_iter(iter)`](https://docs.rs/ratatui/0.30.0/ratatui/text/struct.Text.html#method.from_iter) - render-boundary wrapping for selected transcript rows (`src/ui/render/mod.rs:337`)
- [`Text::push_line(line)`](https://docs.rs/ratatui/0.30.0/ratatui/text/struct.Text.html#method.push_line) + [`Text::height()`](https://docs.rs/ratatui/0.30.0/ratatui/text/struct.Text.html#method.height) - fallback message assembly and scroll extent (`src/ui/render/mod.rs:133`)
- [`Line::from(Vec<Span>)`](https://docs.rs/ratatui/0.30.0/ratatui/text/struct.Line.html) - per-row span composition for ANSI-converted and styled transcript rows
- [`Span::styled(str, Style)`](https://docs.rs/ratatui/0.30.0/ratatui/text/struct.Span.html) - span styling throughout transcript renderer
- [`Paragraph::new(Text::from_iter(output_lines))`](https://docs.rs/ratatui/0.30.0/ratatui/widgets/struct.Paragraph.html) - primary task-output display after row windowing (`src/ui/render/mod.rs:342`)
- [`Paragraph::new(body).scroll((row, 0))`](https://docs.rs/ratatui/0.30.0/ratatui/widgets/struct.Paragraph.html) - fallback message display with a local scroll offset (`src/ui/render/mod.rs:150`)
- [`ansi_to_tui::IntoText`](https://docs.rs/ansi-to-tui) - ANSI escape sequence to ratatui `Text` conversion (`src/ui/render/transcript.rs`)
- [`Style::new().add_modifier(Modifier::X)`](https://docs.rs/ratatui/0.30.0/ratatui/style/struct.Style.html#method.new) - inline styling for tool output, diff lines, and evidence rows
- [`serde_json`](https://docs.rs/serde_json) - incremental ToolCall delta parsing
