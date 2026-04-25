# ADR-013: TUI Completion and Deployment Plan

**Status:** Accepted (all phases complete)  
**Chain:** ADR-009, ADR-010, ADR-011, ADR-012

## Decision

- Full-screen TUI activated via `--ui` flag; batch/headless paths remain unchanged.
- Deployment gated on passing `tests/tui_integration.rs` with a real console emulator.
- All phases complete; see `adr/completed/` records for archived and deprecated design context.
