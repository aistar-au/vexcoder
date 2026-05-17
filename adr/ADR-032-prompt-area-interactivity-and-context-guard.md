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

- [`Paragraph::new().style().wrap(Wrap { trim: false })`](https://docs.rs/ratatui/0.30.0/ratatui/widgets/struct.Paragraph.html) — composer text body (`src/ui/render/mod.rs:render_input_with_actions`)
- [`frame.set_cursor_position((x, y))`](https://docs.rs/ratatui/0.30.0/ratatui/struct.Frame.html#method.set_cursor_position) — cursor placement within the composer (`src/ui/render/mod.rs:118`)
- [`frame.render_widget(Clear, area)`](https://docs.rs/ratatui/0.30.0/ratatui/widgets/struct.Clear.html) + [`Block::default().borders(Borders::ALL)`](https://docs.rs/ratatui/0.30.0/ratatui/widgets/struct.Block.html) — picker overlay modal pattern (`src/ui/render/mod.rs:render_picker_overlay`)
- [`Line::styled(str, style)`](https://docs.rs/ratatui/0.30.0/ratatui/text/struct.Line.html) + `Modifier::DIM` — focus/unfocused state distinction in composer render (`src/ui/render/mod.rs:104`)
- [`Layout::default().direction(Direction::Vertical).constraints([...]).split(area)`](https://docs.rs/ratatui/0.30.0/ratatui/layout/struct.Layout.html) — composer/action-row area split (`src/ui/render/mod.rs:62`); `.split()` is the legacy form; `.areas()` is the 0.28+ replacement
- [`Paragraph::new(visible_actions).style(Style::default().bg(...)).alignment(Alignment::Left)`](https://docs.rs/ratatui/0.30.0/ratatui/widgets/struct.Paragraph.html) — action row above composer (fork shortcut line)
- [RFC 7231 §6.5.1](https://www.rfc-editor.org/rfc/rfc7231#section-6.5.1) — HTTP 400 Bad Request
