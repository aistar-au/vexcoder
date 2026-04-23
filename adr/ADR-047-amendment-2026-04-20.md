# ADR-047 Amendment (2026-04-20): RuntimeEnvelope API Normalization and Consumer Boundary

**Status:** Amended  
**Amends:** ADR-047, ADR-047-amendment-2026-04-16

## Amendment

- Downstream of the API boundary, `RuntimeEnvelope` is the only internal stream contract; server SSE is a transport wrapper over envelope JSON.
- Provider compatibility grammars are confined to immediate ingress; no downstream consumer handles provider-native formats.
- PRs #402–404 established the envelope-only boundary; block-delta and choices-delta conversion removed from server-side.
- Legacy server-side conversion paths are removed; backends and consumers read envelopes directly.
