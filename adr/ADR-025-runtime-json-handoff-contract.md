# ADR-025: Runtime JSON Handoff Contract

**Status:** Complete  
**Chain:** ADR-023, ADR-024, ADR-028

## Decision

- `RuntimeEnvelope` is the normalized internal event type produced at the ingress boundary.
- JSON handoff serializes `RuntimeEnvelope` to JSONL for cross-process transport.
- All consumers receive envelopes; no consumer inspects provider-native event formats.
- Complete; see ADR-047 amendments for subsequent envelope extensions.
