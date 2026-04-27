# ADR-047 Amendment (2026-04-16): API-First Runtime Event Envelope and Trait Reduction

**Status:** Amended  
**Amends:** ADR-047

## Amendment

- `RuntimeEnvelope` is the primary internal API; consumed by transports, UI, persistence, and peer coordination.
- Envelope additions: `frame_id`, `emitted_at`, `source`, optional `request_id`, `parent_frame_id`.
- Explicit tool lifecycle events: `tool_call_started`, `tool_call_arguments_delta`, `tool_call_output_delta`, `tool_call_completed`, `tool_call_failed`.
- Transcript block events remain renderer-oriented; not the primary tool-lifecycle API.
- Requests carry `request_id`; fire-and-forget notifications are distinct.
- `RuntimeMode` and `FrontendAdapter` traits are simplification targets after envelope schema is stable.

## References

- [`serde_json`](https://docs.rs/serde_json) — envelope serialization
