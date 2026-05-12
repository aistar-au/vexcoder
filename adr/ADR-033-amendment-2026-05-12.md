# ADR-033 Amendment (2026-05-12): Phase 2.5 — Locate-then-Edit Cache

**Status:** Amended
**Amends:** ADR-033
**Branch:** `work/vexcoder-edit-location-cache`

## Context

ADR-033 phases shipped a ranked retrieval pipeline (`codebase_search` → `file:line` references) and a context-condensing compaction pass, but the gap between locate and edit was never closed: every `edit_file` call still went straight to disk for its `old_str` match, with no awareness that the same file had just been read in the same pulse. The result was a redundant `read_file` between search and edit on the happy path, and an unrecoverable "must be unique" error on any file with multiple matches outside the model's actual area of interest.

This amendment closes that gap by promoting the existing in-process file cache (ADR-038) and the search tool's existing `file:line` outputs into a single per-pulse ledger that `edit_file` consults for anchored matching.

## Amendment

### Phase 2.5 — Locate-then-Edit Cache

Adds two cooperating mechanisms between Phase 1 retrieval and Phase 3 routing:

1. **Per-pulse `LocatedRead` ledger.** When `read_file` returns through the tool router, the router records a `LocatedRead { path, start_line, end_line, fingerprint }` entry against a process-global mutex-guarded ledger. The fingerprint is the existing `FileFingerprint` (length + mtime) reused from the file cache, so stale entries self-invalidate on the next access — matching the "drop leaked snapshots" posture from ADR-023's compaction work.
2. **Anchored unique match in `edit_file`.** When `old_str` matches multiple times in a file, the operator queries the ledger for an entry whose fingerprint still matches the file on disk. If found, candidate matches are restricted to a window of ±3 lines around the recorded `[start_line, end_line]`. A single in-window candidate succeeds; otherwise the call falls back to the existing full-file uniqueness rule. The single-occurrence happy path is unchanged.

The ledger is cleared at the start of every pulse (`send_message_with_policy` boundary) so that anchors never leak across user turns. Compaction events that fire mid-pulse are transparent to the ledger because it is process-global and stores only paths, ranges, and fingerprints — no message content.

### Scope

- **In:** `read_file` cache wiring, `LocatedRead` ledger, ±3-line anchored match in `edit_file`.
- **Out:** semantic / embedding index; AST-driven refactor; new diff format; explicit `EditAnchor` as a model-visible tool input; cross-pulse / cross-session persistence; agent profiles.

### Invariants preserved

- `edit_file` still refuses full-file replacements, empty `old_str`, and oversized snippets.
- A single full-file match still wins immediately; the anchor only narrows when there is ambiguity.
- File-cache budgets (`MAX_CACHE_FILE_BYTES`, `MAX_CACHE_TOTAL_BYTES`, LRU eviction) are unchanged; files outside the cache budget still read directly from disk and still record ledger entries.
- The codebase symbol index (`CODEBASE_INDEX`) is unaffected.

### Token-savings rationale

The largest, most reliable win is in the drift-retry case: an `edit_file` whose `old_str` previously hit "appears N times; must be unique" now resolves on the first attempt when the search result's line range singularises the occurrence, saving one full retry turn per such edit. The happy-path read remains, but the underlying file content is now served from the in-process cache on its second access, eliminating disk I/O. Wall-clock and token shape mirrors the bounded-message-count reduction probe published with the ADR-023-amendment-2026-05-03 follow-up.

## References

- ADR-023 — deterministic edit loop (uniqueness rule).
- ADR-038 — memory-first in-process file cache.
- ADR-050 — read-file offset carry-forward across compaction (companion metadata pattern).
