# ADR-048: Operator Permissions Overlay and Mode Precedence

**Status:** Proposed  
**Chain:** ADR-022, ADR-024 Gap 13, ADR-042, ADR-038

## Context

Permission evaluation was scattered across `RuntimeCorePolicy`, privacy disclosure, and tool registration. This ADR formalizes the evaluation order and protected-path invariants before enforcement code lands.

## Decision

- Permissions overlay is distinct from privacy disclosure and `RuntimeCorePolicy`.
- `src/tools/operator/policy.rs` is the reserved evaluation boundary for the overlay.
- Four permission modes: `Default`, `AcceptEdits`, `Plan`, `BypassPermissions`.
- Evaluation order per request: protected-path gate → deny rules → allow rules → mode default.
- Protected paths (`.vex/state/`, `.vex/index/`, `.vex/`, credentials files) binding in every mode.
- Settings precedence: enterprise policy → user → trusted project → defaults.
- Untrusted workspace demotes to default posture regardless of project config.
- Interactive prompts are non-blocking; non-interactive mode fails closed on ambiguous requests.
- Introspection required: `/permissions`, `vex permissions`, `GET /v1/permissions`, structured evidence in tool approval output.

## References

- [`serde`](https://docs.rs/serde) — permission config deserialization
