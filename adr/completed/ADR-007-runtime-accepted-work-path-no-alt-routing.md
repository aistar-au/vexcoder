# ADR-007: Accepted Work Path — No Alternate Routing

**Status:** Accepted  

## Decision

- There is one work path: `RuntimeContext::start_pulse`.
- No alternate routing via conditional branches on mode or capability flags.
- Guards and approval layers insert into the single path; they do not fork it.
