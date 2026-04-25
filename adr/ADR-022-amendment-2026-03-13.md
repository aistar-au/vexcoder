# ADR-022 Amendment (2026-03-13)

**Status:** Amended  
**Amends:** ADR-022

## Amendment

- Capability-based approval granted per `Capability` variant, not per tool name.
- `ApprovalScope::Task` grants for the session; `ApprovalScope::Once` grants for one invocation.
- Both scopes require explicit user action; no auto-escalation on repeated denial.
