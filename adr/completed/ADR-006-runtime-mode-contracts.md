# ADR-006: Runtime Mode Contracts

**Status:** Accepted  

## Decision

- `RuntimeMode` enum variants: `Batch`, `Interactive`, `Headless`.
- Each mode variant owns its event-loop contract; no cross-mode behavior sharing.
- `RuntimeCorePolicy` enforces the base prompt and approval chain independent of mode.
- `RuntimeContext::start_turn` is the sole dispatch path after REF-05.
