# ADR-028: Application Facade and Transport Boundaries

**Status:** Active (Phase 1 + 2 merged; boundary tests cover grouped, multiline, and `super::`-relative imports)  
**Chain:** ADR-018, ADR-019, ADR-024, ADR-025, ADR-026, ADR-027

## Context

Transport logic, application logic, and CLI concerns were entangled across `src/app.rs` and `src/server.rs`, making independent replacement of any layer impractical.

## Decision

- Adopt strict inward dependency direction: CLI → Facade; Transport → Facade; Facade → Orchestration → Runtime core.
- Transport layer owns HTTP/SSE/socket binding and protocol negotiation; no runtime logic.
- Application facade exposes a transport-agnostic API; callers pass high-level commands, not raw requests.
- Facade reuses `RuntimeEnvelope` (ADR-025); no second internal event schema.
- CLI layer is a thin wrapper around config loading and facade invocation.
- Orchestration/agent-loop normalizes provider-native events into shared runtime envelopes before any downstream consumption.
- Dependency-direction tests (`tests/dependency_direction_tests.rs`) enforce no upward imports.

## References

- [`axum`](https://docs.rs/axum) — HTTP transport (MIT)
- [`tokio`](https://docs.rs/tokio) — async runtime
- [RFC 8441](https://www.rfc-editor.org/rfc/rfc8441) — HTTP/2 WebSocket support
