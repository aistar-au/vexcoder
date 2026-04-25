# ADR-022: Free/Open Coding Agent Roadmap

**Status:** Proposed (Phase 1 validation passed 2026-03-15; Phases G–H pending)  
**Chain:** ADR-014, ADR-018, ADR-020, ADR-021  
**See also:** ADR-023 (deterministic edit loop), ADR-024 (parity gaps)

## Context

`vexcoder` targets release as a coding agent whose runtime and packaging dependencies carry exclusively permissive, no-cost licenses. This ADR defines the first-release feature scope and eight-phase source-transformation sequence from the pre-release baseline.

## Decision

- CLI-agent-first for first release; editor integrations and native GUI deferred.
- Default posture: approval-required for all mutating tools.
- Local runtime and self-hosted server support mandatory; no vendor lock-in in config keys or validation.
- Edits are diff-native and approval-gated (two-step `propose_patch` → `apply_patch`).
- Command execution is a first-class built-in capability, not a plugin.
- Capability-based approval tracks `Capability` variants, not raw tool names.
- Remove all provider-branded config keys and defaults from `Config`; use neutral names.
- Eight-phase restructuring order: neutralize config → command execution → diff-native writes → approval policy → durable task state → TUI rework → repo tools → defer browser/editor.

## References

- [`serde`](https://docs.rs/serde) — config deserialization
- [`tokio`](https://docs.rs/tokio) — async runtime (Apache-2.0/MIT)
- [`ratatui`](https://docs.rs/ratatui) — TUI framework (MIT)
