# ADR-040: Real-Time Local Turn Telemetry

**Status:** Proposed (operator-surface contract corrected 2026-04-09)  
**Chain:** ADR-030, ADR-031, ADR-038, ADR-039

## Context

Local-server chat-compatible streams returned progress and timing metadata that the runtime discarded. Messages/v1 streams carry the same data natively.

## Decision

- Local chat-compatible requests include `return_progress: true` and `timings_per_token: true` where supported.
- Messages/v1 requests receive prompt progress and token timing without additional opt-in.
- Metadata-only stream chunks (null content) are valid turn-progress signals; not dropped.
- Shared types: `StreamPromptProgress`, `StreamTimings`, `ApiUsage` across both protocol paths.
- Stream parser attempts messages/v1 first; falls back to chat-compatible on 404/415.
- `Mapping adjacent sectors...` phrase appends telemetry as a suffix without replacement.
- Owned transcript surface carries both committed progress and live in-flight progress.
- Status bar priority order: task identity > mode/approval/model > timing > branch/counters.

## References

- [`tokio-stream`](https://docs.rs/tokio-stream) — async stream processing
