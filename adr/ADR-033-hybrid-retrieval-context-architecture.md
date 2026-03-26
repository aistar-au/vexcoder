# ADR-033: Hybrid Retrieval Context Architecture

- **Status:** Active
- **Date:** 2026-03-22
- **Deciders:** Core maintainer
- **Depends on:** ADR-032, ADR-031, ADR-029
- **Supersedes:** None
- **Superseded by:** None

## Context

ADR-032 introduced context-proportional `read_file` with offset/limit and an
auto-cap derived from `VEX_MAX_TOKENS`. This prevents naive full-file reads
from exhausting small context windows, but the approach is still line-count
based: the model must guess offsets, cannot search semantically, and large
codebases (thousands of files, multi-thousand-line modules) still produce
excessive context consumption when the model reads sequentially.

Production-grade agentic coding tools solve this with a hybrid retrieval
pipeline: AST-aware structural indexing, semantic vector search, and
on-demand snippet retrieval. The model never reads a full file unless
explicitly instructed; instead it queries an index and receives ranked
snippets with file:line references.

## Decision

### Phase 1 — Structural search tool (`codebase_search`)

1. Add a `codebase_search` tool to the tool registry. The tool accepts a
   natural-language query and returns ranked code snippets (function bodies,
   impl blocks, type definitions) with file path and line range.

2. Build a Tree-sitter-based structural index for Rust source files:
   - Parse each `.rs` file in the workspace into an AST.
   - Extract named items: functions, structs, enums, impls, traits, modules,
     const/static declarations, type aliases.
   - Store each item as a chunk with: path, start line, end line, item kind,
     item name, parent scope, raw source text.
   - Index is built at session start and updated incrementally on file writes.

3. Search ranking uses a combination of:
   - Exact name match (highest weight).
   - Substring / fuzzy match on item names and parent scopes.
   - Content keyword match within chunk bodies.
   - Recency: recently written/edited chunks rank higher.

4. Results are capped at a configurable `VEX_SEARCH_MAX_RESULTS` (default 10)
   and `VEX_SEARCH_MAX_TOKENS` (default: 10% of `VEX_MAX_TOKENS`).

### Phase 2 — Semantic vector search (optional, additive)

5. When an embedding provider is configured (`VEX_EMBEDDING_PROVIDER`,
   `VEX_EMBEDDING_MODEL`, `VEX_EMBEDDING_URL`), chunks from Phase 1 are
   embedded at logical boundaries (function/type level) and stored in a
   persisted local vector index under `.vex/index/semantic-codebase-index.json`.

6. Semantic search returns snippets ranked by cosine similarity to the query
   embedding, merged with structural match scores from Phase 1. When no
   embedding provider is configured, `codebase_search` remains structural-only.

7. The vector index is updated incrementally and first-time indexing is bounded
   by `VEX_INDEX_MAX_FILES` (default 5000).

### Phase 3 — Diff-native edits and write guards

8. `apply_diff` becomes the preferred edit path for files exceeding a
   configurable threshold (`VEX_DIFF_PREFERRED_ABOVE_LINES`, default 200).
   When the model calls `write_file` on a file above this threshold, the
   tool emits a warning suggesting `apply_diff` instead.

9. `write_file` on files exceeding `VEX_WRITE_FILE_MAX_LINES` (default 500)
   is rejected with an error directing the model to use `apply_diff` or
   `edit_file`. This prevents truncation on large files.

### Phase 4 — Context condensing

10. After each model turn, conversation history older than the last N turns
    (configurable via `VEX_HISTORY_KEEP_TURNS`, default 10) is summarized
    into a compact context block. The summarized block replaces the original
    messages, preserving key decisions and file state while freeing tokens.

11. Tool results from completed turns are truncated to their first 5 lines
    plus a `(N more lines)` indicator, since the model has already acted on
    the full content.

### Integration with existing tools

12. The system prompt instructs the model to prefer `codebase_search` over
    `read_file` for exploration, and to use `read_file` with `offset`/`limit`
    only when the exact location is already known.

13. `@path:function_name` in prompt input resolves via the structural index
    to the specific function body, not the entire file.

## Consequences

- Models consume an order of magnitude fewer tokens per exploration cycle.
- Large files (1K+ lines) are navigable without reading them in full.
- `apply_diff` preference prevents truncation on large-file edits.
- Context condensing extends effective session length on small-context servers.
- Phase 1 (structural search) requires Tree-sitter Rust parser as a build
   dependency. Phase 2 (vector search) is optional, requires an embedding
   provider, and persists provider/model-specific embeddings inside `.vex/index/`.

## Implementation order

Phase 1 is the minimum viable delivery: Tree-sitter structural index +
`codebase_search` tool. Phases 2-4 are additive and can land independently.

## Implementation status

All four phases are implemented on `main` as of 2026-03-26.

| Phase | Feature | Key source files |
| :--- | :--- | :--- |
| 1 | Structural search (`codebase_search`) | `src/api/client.rs` (tool definition), `src/state/conversation/tools.rs` (index lifecycle) |
| 2 | Semantic vector search | `src/state/conversation/tools.rs` (embedding config readers) |
| 3 | Write guards | `src/state/conversation/tools.rs` (`write_file_diff_preferred_above_lines`, `write_file_max_lines`), `src/api/client.rs` (tool description) |
| 4 | Context condensing | `src/state/conversation/history.rs` (`condense_old_tool_results`, `compact_for_context_overflow`), `src/api/client.rs` (system prompt guidance) |

System prompt guidance and tool descriptions now reference all four phases.

## References

- [ADR-032](https://github.com/aistar-au/vexcoder/blob/main/adr/ADR-032-prompt-area-interactivity-and-context-guard.md) — prompt area interactivity and context guard
- [ADR-029](https://github.com/aistar-au/vexcoder/blob/main/adr/ADR-029-stream-parser-completeness-and-session-persistence.md) — stream parser and session persistence
- [ADR-031](https://github.com/aistar-au/vexcoder/blob/main/adr/ADR-031-operator-surface-ui-overhaul.md) — operator surface UI overhaul
