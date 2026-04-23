# ADR-031: Operator Surface UI Overhaul

**Status:** Accepted (Batches A–E merged; 2026-04-08 host-scrollback amendment deprecated 2026-04-09)  
**Chain:** ADR-022, ADR-027, ADR-028, ADR-030

## Context

The pre-ADR-031 TUI used a fixed layout with no scrolling transcript and mixed ownership between a host scrollback buffer and the application. The overhaul establishes a single app-owned transcript surface.

## Decision

- Adopt task-state-first fullscreen layout: scrolling transcript (app-owned), persistent composer, status bar.
- Transcript is the authoritative visible stream for status, tool activity, approvals, and orchestrator updates.
- Scroll ownership moves to the task surface; viewport starts at top on session open.
- Compact telemetry: six-line inspector cap; enriched tool paragraphs show 3 evidence lines + overflow indicator.
- Cross-platform resize robustness with 10×4 minimum viable surface.
- Detail views use overlays and pagers, not permanent activity strips.
- Batch development may proceed in parallel but batches merge in dependency order (state-first, then UI).
- No `HostScrollbackSink` or ratatui inline viewport insertion; owned transcript only.

## References

- [`ratatui`](https://docs.rs/ratatui) — TUI framework
- [`crossterm`](https://docs.rs/crossterm) — console backend
