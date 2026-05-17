# ADR-010: TUI Viewport and Transcript

**Status:** Accepted  
**See also:** ADR-031 (overhaul), ADR-027 (full-screen)

## Decision

- Transcript is append-only; no retroactive mutation.
- Scroll position is managed manually by the application: `Paragraph::scroll((row as u16, 0))`
	is called with an offset computed in `src/app/scroll.rs`. There is no widget-managed
	`ListState` or `ScrollbarState`; no visual `Scrollbar` widget is rendered. The scroll
	offset is clamped to content height via `apply_bounded_scroll`.
- No `Scrollbar` widget is rendered alongside the transcript; scroll position is not visually
	indicated. This is a known gap tracked in `TASKS/PR-400-ratatui-api-surface-map.md`.
- Deprecated: host-scrollback model (see ADR-027, ADR-031).
