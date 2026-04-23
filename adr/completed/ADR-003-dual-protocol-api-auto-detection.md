# ADR-003: Dual-Protocol API Auto-Detection

**Status:** Accepted  
**See also:** ADR-047 (block-delta extension)

## Decision

- Runtime probes `POST /v1/messages` first; falls back to `POST /v1/chat/completions` on 404/415.
- Detection result cached for the session; `explicit_protocol` config key overrides.
- Both protocol paths normalize to `RuntimeEnvelope` at ingress.

## References

- [RFC 7231 §5.3.2](https://www.rfc-editor.org/rfc/rfc7231#section-5.3.2) — HTTP `Accept` header
