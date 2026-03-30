# Task PM-03: Code Search and Semantic Indexing

**Target Files:** `src/tools/index.rs`, `src/tools/search.rs`, `src/tools/semantic.rs`, `src/state/conversation/tools.rs`, `src/tools/operator.rs`, `src/app/commands.rs`, `src/config.rs`, `src/config/load.rs`, `src/app.rs`, `src/app/ctor.rs`, `src/api/client.rs`, `src/app/tests/memory.rs`, `src/app/tests/session.rs`, `src/batch_mode.rs`, `src/state.rs`, `src/state/conversation.rs`, `tests/integration_test.rs`, `tests/live_server_test.rs`

**Depends on:** None (green on current main)

---

## Issue

The agent already exposes `codebase_search` backed by a lazy structural index
plus optional semantic reranking. The current search stack is still limited:

1. Structural indexing is hard-coded around the current Rust source tree.
2. There is no operator-facing `/reindex` command for out-of-band file
  changes.
3. Search limits, exclusions, and rebuild policy are not configurable.
4. The index lifecycle is implicit across several files, which makes behavior
  harder to reason about and tune.

This branch should harden and extend the existing search surface rather than
introducing a second parallel search tool.

---

## Decision

### Indexing strategy

Keep the existing structural-plus-semantic design and extend it with explicit
index lifecycle controls. Structural search remains Rust-aware, while semantic
reranking continues to use the persisted embedding cache when configured.

### Index lifecycle

1. **Build**: Keep the current lazy build for `codebase_search`, but add an
  explicit `/reindex` command and configuration for eager rebuild behavior.
2. **Query**: Keep `codebase_search` as the operator-facing tool surface.
3. **Incremental update**: Continue refreshing affected entries after
  file-modifying tool calls, and document the contract explicitly.
4. **Persistence**: Keep semantic cache persistence under `.vex/index/`, and
  make rebuild/reset rules explicit in config and tests.

### Existing tool surface: `codebase_search`

```
codebase_search(query: string, max_results: int = 20) -> Vec<SearchResult>
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

- No external binaries or services. Indexing must remain pure Rust and part of
  the main binary.
- No network calls. Indexing is strictly local.
- Do not add a second search tool name. Extend `codebase_search` instead.
- Keep semantic cache persistence under `.vex/index/`; do not introduce a
  second persistence location for the same search surface.
- Do not index binary files when broadening search scope.
- Must not block the main event loop. Indexing runs on a background task
  and the agent can use other tools while indexing is in progress.
- `/reindex` must reuse the existing indexing helpers instead of duplicating
  rebuild logic.
- Must not regress existing tests.

---

## Definition of Done

1. `codebase_search` remains registered and callable by the agent.
2. Search config controls index scope, exclusions, and rebuild behavior.
3. Files modified by tool calls continue to trigger incremental index updates.
4. `/reindex` forces a full structural rebuild and refreshes semantic cache
  state when needed.
5. Binary files and files exceeding `max_file_size` are skipped when broader
  indexing is enabled.
6. `cargo test --all-targets` is green.

---

## Anchor Tests

`test_codebase_search_tool_returns_ranked_results`
`test_update_index_replaces_file_chunks`
`test_reindex_rebuilds_full_index`
`test_search_config_respects_exclude_paths`
`test_incremental_update_after_write_file`
`test_search_config_loads_from_both_layers`

Primary verification anchor:

```rust
#[test]
fn test_codebase_search_tool_returns_ranked_results() {
  // Given a rebuilt codebase index with known symbols,
  // querying via codebase_search must return ranked results for the
  // matching item names and snippets.
}
```
