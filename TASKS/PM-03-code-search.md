# Task PM-03: Code Search and Semantic Indexing

**Target Files:** `src/tools/operator.rs`, `src/tools/mod.rs`,
`src/state/conversation/tools.rs`, `src/config.rs`, `src/config/load.rs`,
`src/indexer.rs` (new), `src/indexer/trigram.rs` (new)

**Depends on:** None (green on current main)

---

## Issue

The agent's only file-discovery mechanism is directory listing and
regex-based grep. For large codebases, the agent must guess file paths or
exhaustively scan directories. This leads to:

1. Wasted tool calls on path-finding before the agent can read the right file.
2. Missed relevant files when naming conventions are non-obvious.
3. No structural awareness (e.g., find all callers of a function).

A local indexing system would let the agent search by content, symbol, or
semantic similarity without scanning the full tree on every query.

---

## Decision

### Indexing strategy

Implement a trigram-based index as the initial backend. Trigram indexing is
language-agnostic, requires no external dependencies, and supports
substring matching with reasonable performance for repositories up to ~100k
files.

### Index lifecycle

1. **Build**: On session start (or on demand via `/reindex`), walk the
   project tree and build an in-memory trigram index. Respect `.gitignore`
   and a configurable exclude list.
2. **Query**: The agent calls a new `code_search` tool with a query string.
   The index returns matching file paths ranked by trigram hit count.
3. **Incremental update**: After file-modifying tool calls, update the index
   entries for affected files.

### New tool: `code_search`

```
code_search(query: string, max_results: int = 20) -> Vec<SearchResult>
```

Returns `{ path, line_number, snippet, score }` for each match.

### Configuration surface

```toml
# .vex/config.toml or ~/.config/vex/config.toml

[search]
enabled       = true        # default: true
auto_index    = true        # index on session start
exclude       = ["target/", "node_modules/", ".git/"]
max_file_size = 1048576     # skip files larger than 1 MiB
```

### `/reindex` slash command

Force a full re-index of the project tree. Useful after external changes
(e.g., `git checkout` outside the agent session).

---

## Constraints

- No external binaries or services. The indexer must be pure Rust and
  compiled into the main binary.
- No network calls. Indexing is strictly local.
- Index is in-memory only. Not persisted to disk (persistence is a future
  enhancement).
- Do not index binary files. Use the same heuristic as `grep`: skip files
  with null bytes in the first 8 KiB.
- Must not block the main event loop. Indexing runs on a background task
  and the agent can use other tools while indexing is in progress.
- The `code_search` tool must be registered in the tool dispatch table and
  appear in tool listings.
- Must not regress existing tests.

---

## Definition of Done

1. `code_search` tool is registered and callable by the agent.
2. Trigram index is built from the project tree on session start (when
   `auto_index = true`).
3. `.gitignore` patterns and `exclude` config are respected.
4. Files modified by tool calls trigger incremental index updates.
5. `/reindex` forces a full rebuild.
6. Binary files and files exceeding `max_file_size` are skipped.
7. `cargo test --all-targets` is green.

---

## Anchor Tests

`test_trigram_index_finds_substring_match`
`test_trigram_index_respects_gitignore`
`test_trigram_index_skips_binary_files`
`test_code_search_tool_returns_ranked_results`
`test_incremental_update_after_write_file`
`test_reindex_rebuilds_full_index`
`test_search_config_loads_from_both_layers`

Primary verification anchor:

```rust
#[test]
fn test_trigram_index_finds_substring_match() {
    // Given a trigram index built from a directory with known file content,
    // querying for a substring present in one file must return that file
    // with a non-zero score.
}
```
