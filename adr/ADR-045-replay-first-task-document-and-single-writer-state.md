# ADR-045: Replay-First Task Document and Single-Writer State

**Status:** Proposed  
**Chain:** ADR-025, ADR-029, ADR-030, ADR-035, ADR-041, ADR-043

## Context

`TaskState` fields were written from multiple sites including direct provider event handlers, creating non-deterministic state that could not be reliably replayed from the event log.

## Decision

- Adopt replay-first, single-writer architecture: `TaskDocument` is always a deterministic projection of an append-only `RuntimeEventLog`.
- `TaskDocumentCondenser` is the sole writer to replay-relevant `TaskDocument` fields.
- Provider-native events are never appended to the event log directly; all events pass through normalization first (ADR-030).
- Every `TaskDocument` field must have a corresponding `RuntimeSignal` variant and a condenser handler.
- `TaskDocumentCheckpoint` captures all replay-relevant fields for full-fidelity session resume.
- Rollback is represented as an appended marker event, not log truncation.
- Seven component boundaries: `EventLog`, `Condenser`, `TaskDocument`, `ConversationManager`, `CondensationSummary`, `Projections`, `Checkpoint`.
- Deprecates lossy `persistable_snapshot` as an accepted resume source.

## References

- [`serde_json`](https://docs.rs/serde_json) — event log serialization
- [`tokio`](https://docs.rs/tokio) — async condenser pipeline
