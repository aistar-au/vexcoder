# ADR-006: Runtime Mode Contracts

**Status:** Accepted  

## Decision

- `RuntimeMode` is a trait in `src/runtime/mode.rs`; concrete modes implement the trait rather than selecting enum variants.
- `InputOccurrence` carries typed frontend events, including text, interrupt, and scroll actions.
- Each mode implementation owns its event-loop behavior; prompt and approval policy remain mode-independent in `RuntimeCorePolicy`.
- `RuntimeContext::start_pulse` is the sole work path after REF-05.
