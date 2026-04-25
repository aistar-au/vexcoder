# ADR-027: Full-Screen TUI Command-Session Capture

**Status:** Accepted (complete; deprecates ADR-018, ADR-019)  
**Chain:** ADR-013, ADR-018, ADR-019

## Decision

- Full-screen TUI captures all command output in the app-owned transcript; no host scrollback.
- Deprecates `HostScrollbackSink` (ADR-018) and the ADR-019 follow-up correctness pass.
- All transcript rendering routes through the owned-surface path (ADR-031).
