# ADR-014: Policy Dedup and Enforcement

**Status:** Accepted  

## Decision

- `RuntimeCorePolicy` is the single enforcement point for base prompt, model parameters, and approval chain.
- Duplicate policy checks removed from individual tool handlers; all checks delegate to `RuntimeCorePolicy`.
