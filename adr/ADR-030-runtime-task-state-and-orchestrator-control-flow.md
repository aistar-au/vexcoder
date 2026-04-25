# ADR-030: Runtime Task State and Orchestrator Control Flow

**Status:** Accepted (invariant fixes merged 2026-03-17; verification 2026-03-28)  
**Chain:** ADR-023, ADR-025, ADR-027, ADR-028, ADR-029

## Context

Task truth was divided between provider event order and in-memory state, causing the application facade to act as orchestrator and allowing UI components to bypass task state.

## Decision

- Runtime is the task-state-owned orchestrator: provider event → normalize → update task state → orchestrator decides next action.
- Provider events are never task truth; stream completion does not end the task.
- Managed command sessions outlive individual provider stream chunks.
- Tool and command results re-enter task state before the next model turn.
- Downstream consumers (UI, batch, export) read shared `RuntimeEnvelope` events only; not provider-native names.
- Application facade is not the orchestrator; it delegates to the runtime.
- Six mandatory invariants: (1) task state owns truth, (2) provider events normalized at ingress, (3) orchestrator gates next turn, (4) tool results update state before retry, (5) UI reads envelopes, (6) facade delegates.

## References

- [`tokio`](https://docs.rs/tokio) — async task runtime
- [`serde_json`](https://docs.rs/serde_json) — envelope serialization
