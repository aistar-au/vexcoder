# ADR-008: Runtime Cutover Parity Guardrails

**Status:** Accepted  

## Decision

- Each runtime cutover phase gates on a parity checklist: behavior tests pass against both old and new paths.
- No phase ships until all items on the parity checklist are verified green.
- Checklist items are tracked in the relevant ADR's implementation section.
