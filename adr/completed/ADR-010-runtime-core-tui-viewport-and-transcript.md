# ADR-010: TUI Viewport and Transcript

**Status:** Accepted  
**See also:** ADR-031 (overhaul), ADR-027 (full-screen)

## Decision

- Transcript is append-only; no retroactive mutation.
- Viewport scroll position is managed by the TUI widget, not the application state.
- Deprecated: host-scrollback model (see ADR-027, ADR-031).
