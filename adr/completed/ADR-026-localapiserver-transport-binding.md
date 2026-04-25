# ADR-026: LocalApiServer Transport Binding

**Status:** Complete  
**Chain:** ADR-025, ADR-028

## Decision

- `LocalApiServer` binds to `127.0.0.1:0`; assigned port reported via stdout at startup.
- HTTP bodies use `application/json`; SSE responses use `text/event-stream`.
- Server lifetime tied to the owning runtime process; no daemonization.

## References

- [RFC 7230](https://www.rfc-editor.org/rfc/rfc7230) — HTTP/1.1 message syntax
- Server-Sent Events: <https://html.spec.whatwg.org/multipage/server-sent-events.html>
