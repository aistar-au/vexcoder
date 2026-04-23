# ADR-043: Structured Output Parser Adoption Gates

**Status:** Active (adoption gates open; not default path)  
**Chain:** ADR-029, ADR-030, ADR-031, ADR-041

## Context

An alternative structured-output parser exists in the tree but is not the default runtime path. Premature promotion risks behavioral regression in transcript fidelity.

## Decision

- Primary live parser path remains shared ingress + runtime normalization (ADR-029 / ADR-030).
- No alternative parser becomes default without satisfying all three adoption gates:
  - **Gate 1:** At least one production runtime path routes structured-parser decisions through live conversation.
  - **Gate 2:** Test suite demonstrates behavioral parity against the current parser path.
  - **Gate 3:** Captured malformed fixtures show measurable reduction in transcript loss or decode ambiguity.
