# ADR-033: Hybrid Retrieval Context Architecture

**Status:** Accepted (all four phases merged)  
**Chain:** ADR-032, ADR-031, ADR-029

## Context

Unstructured `read_file` calls were the sole context-gathering mechanism, producing oversized context windows for large repos. A phased retrieval pipeline replaces full-file reads with ranked structural results.

## Decision

- Phase 1: `codebase_search` tool indexes Rust symbols via [`tree-sitter`](https://docs.rs/tree-sitter); returns named items (functions, structs, enums, impls) with `file:line` references.
- Search ranking: name exact match → substring/fuzzy → content keywords → recency.
- Results capped at `VEX_SEARCH_MAX_RESULTS` (default 10) and `VEX_SEARCH_MAX_TOKENS`.
- Phase 2: Optional vector embeddings when provider is configured; not required for operation.
- Phase 3: `apply_diff` preferred for files >200 lines; `write_file` rejected for files >500 lines.
- Phase 4: Context condensing — summarize conversation history older than N turns via a compaction pass.
- All four phases are accepted and in effect as of 2026-03-26.

## References

- [`tree-sitter`](https://docs.rs/tree-sitter) — structural indexing
- [`tokenizers`](https://docs.rs/tokenizers) — token counting for budget enforcement
