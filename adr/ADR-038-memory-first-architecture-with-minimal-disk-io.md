# ADR-038: Memory-First Architecture with Minimal Disk I/O

**Status:** Accepted (Batches A–H merged)  
**Chain:** ADR-029, ADR-030, ADR-033, ADR-034

## Context

Redundant disk reads during context assembly and speculative `TaskState::load()` calls on startup produced unnecessary I/O latency under low-memory conditions.

## Decision

- Automatic context assembly prefers process-resident caches over disk reads.
- Automatic git status/diff is opt-in via `VEX_CONTEXT_INCLUDE_GIT`; not injected by default.
- Explicit tool-driven reads (`read_file`, `codebase_search`), search indexes, and task-state JSON remain as intended durable surfaces.
- `src/runtime/context_cache.rs` provides a bounded in-process cache for small text files.
- Git helpers isolated to `src/runtime/git_snapshot.rs` via [`gix`](https://docs.rs/gix).
- Config cache at `src/config/cache.rs` avoids repeated TOML parsing.
- Disk-policy enforcement at `src/disk_policy.rs` gates all write paths.

## References

- [`gix`](https://docs.rs/gix) — git object access (MIT)
- [`tokio`](https://docs.rs/tokio) — async I/O scheduling
