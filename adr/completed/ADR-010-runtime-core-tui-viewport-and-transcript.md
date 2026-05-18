# ADR-010: TUI Viewport and Transcript

**Status:** Accepted  
**See also:** ADR-031 (overhaul), ADR-027 (full-screen)

## Decision

- Transcript is append-only; no retroactive mutation.
- Scroll position is managed manually by the application. The primary task-output
  path slices `expanded_output_rows` with `task_output_window_with_total`, then
  renders `Paragraph::new(Text::from_iter(output_lines))`. The fallback
  `render_messages` path is the only current `Paragraph::scroll((row, 0))`
  caller, and its offset is computed locally from `Text::height()` in
  `src/ui/render/mod.rs`.
  Review-scroll math remains in `src/app/scroll.rs`.
- No `Scrollbar` widget is rendered alongside the transcript; scroll position is not visually
  indicated. This is a known gap tracked in `TASKS/PR-400-ratatui-api-surface-map.md`.
- Deprecated: host-scrollback model (see ADR-027, ADR-031).
