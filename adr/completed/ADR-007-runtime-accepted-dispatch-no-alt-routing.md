# ADR-007: Accepted Dispatch — No Alternate Routing

**Status:** Accepted  

## Decision

- There is one dispatch path: `RuntimeContext::start_turn`.
- No alternate routing via conditional branches on mode or capability flags.
- Guards and approval layers insert into the single path; they do not fork it.
