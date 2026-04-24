# ADR-003: Dual-Protocol API Auto-Detection

**Status:** Accepted  
**See also:** ADR-047 (block-delta extension)

## Decision

- Runtime probes `GET /v1/messages` first with `Accept: text/event-stream`; if that endpoint does not return `200 OK` with an SSE content type, it probes `GET /v1/chat/completions` with the same header.
- Detection result cached for the session; `explicit_protocol` config key overrides.
- Both protocol paths normalize to `RuntimeEnvelope` at ingress.

## References

- [RFC 7231 §5.3.2](https://www.rfc-editor.org/rfc/rfc7231#section-5.3.2) — HTTP `Accept` header
- [WHATWG Server-Sent Events](https://html.spec.whatwg.org/multipage/server-sent-events.html) — `text/event-stream`
