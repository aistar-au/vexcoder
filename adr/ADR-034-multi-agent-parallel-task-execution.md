# ADR-034: Multi-Agent Parallel Task Execution

**Status:** Active (Phase A + B–E baseline merged)  
**Chain:** ADR-024, ADR-025, ADR-026, ADR-030, ADR-033

## Context

Orchestrating multiple concurrent agent sessions required explicit lifecycle management, isolation guarantees, and a channel model for inter-agent coordination.

## Decision

- Orchestrator remains sole authority for session-task lifecycle; no agent self-spawns.
- Agent definitions declared in `.vex/agents.toml` (profiles, teams, tool capabilities, isolation policy).
- Worktree isolation mandatory for concurrent code-bearing agents via [`gix`](https://docs.rs/gix) worktree lease.
- One mutable agent per leased worktree; read-only tasks may share the origin worktree.
- Background session tasks are first-class task-state entries with explicit lifecycle metadata.
- Operator surfaces (`/agents`, `/delegate`, `/watch`, `vex tasks`) are observational only; no direct agent mutation.
- Handoff and export reuse ADR-025 `RuntimeEnvelope` and ADR-030 task-state ownership.

## References

- [`gix`](https://docs.rs/gix) — git worktree management (MIT)
- [`tokio`](https://docs.rs/tokio) — async task spawning
