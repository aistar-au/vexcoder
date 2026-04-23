# ADR-012: TUI Deployment Gate

**Status:** Accepted  

## Decision

- TUI activates only when `--ui` flag is present or `VEX_UI=1` is set.
- Gate verified by `tests/tui_integration.rs` with a real console emulator target.
