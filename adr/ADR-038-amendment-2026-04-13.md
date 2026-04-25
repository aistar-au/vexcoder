# ADR-038 Amendment (2026-04-13): Task-State Cold-Start Memory Bounds

**Status:** Accepted  
**Amends:** ADR-038  
**Chain:** ADR-038, ADR-034, ADR-045

## Amendment

- Cap startup task scans at `VEX_MAX_STARTUP_TASK_SCANS` (default 200) to bound cold-start allocation.
- Introduce `TaskStateHeader` as a header-only projection for session-check and UI-list paths.
- `TaskState::load()` must not be called speculatively during cold-start discovery; load only on explicit user selection.
- Bounded in-memory LRU cache for `TaskStateHeader` projections; evict oldest on capacity.
- Promotes existing private `TaskStateLiveSummary` to the public `TaskStateHeader` type.
