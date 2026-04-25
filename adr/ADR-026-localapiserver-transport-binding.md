# ADR-026: LocalApiServer Transport Binding

**Status:** Complete  
**Chain:** ADR-025, ADR-028

## Decision

- `LocalApiServer` binds to `127.0.0.1:0` and reports the assigned port via stdout at startup.
- HTTP requests use `application/json` bodies; SSE responses use `text/event-stream`.
- Server lifetime is tied to the owning runtime process; no daemonization.
- Complete; transport layer is in effect per ADR-028.

## References

- [RFC 7230](https://www.rfc-editor.org/rfc/rfc7230) — HTTP/1.1 message syntax
- Server-Sent Events: <https://html.spec.whatwg.org/multipage/server-sent-events.html>
