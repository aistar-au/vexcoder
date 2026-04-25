# ADR-016: Local Tool-Loop Guard and Correction

**Status:** Accepted  

## Decision

- Tool-call depth at the conversation layer capped at `VEX_MAX_TOOL_CALLS` (default 20).
- On cap hit, the runtime injects a structured correction pulse rather than truncating silently.
- Cap applies per conversation pulse, not per session.
