# ADR-021: Codebase Audit — Unused Code, Duplication, and Shared Code Opportunities

**Status:** Accepted  
**Chain:** ADR-006, ADR-014, ADR-016, ADR-020

## Context

A structured post-ADR-020 audit identified unreachable exports, duplicated parsing logic, and patterns eligible for shared abstractions.

## Decision

- Remove all `pub` items with no external call sites (confirmed via `cargo check --all-targets`).
- Merge duplicate SSE line-parsing logic into a single `src/api/stream.rs` impl.
- Promote `EnvLockGuard` and `EnvRestore` to `src/test_support` as canonical safe env-mutation helpers.
- Unify `StreamTextNormaliser` initialization into one construction site.
- All removals gate on `cargo nextest run` green across all targets.

## References

- [`cargo-nextest`](https://docs.rs/cargo-nextest) — test harness
