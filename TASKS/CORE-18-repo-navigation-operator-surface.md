# Task CORE-18: Repo Navigation Operator Surface

**Target File:** `src/tools/operator.rs`, `tests/tool_operator_tests.rs`

**ADR:** ADR-022 Phase 7

**Depends on:** CORE-16 (`test_approval_policy_read_file_auto_allows_without_prompt` must be green)

---

## Issue

`ToolOperator` provides `read_file` and `list_files` but no content search or symbol
lookup. The agent cannot locate relevant files autonomously without operator guidance.
ADR-022 Phase 7 requires grep-equivalent content search and navigation tools that are
repo-confined and use `Capability::ReadFile` (auto-allowed by default policy).

---

## Decision

1. Add `search_content(query: &str, path_glob: Option<&str>) -> Result<Vec<SearchMatch>>`
   to `ToolOperator`. `SearchMatch` carries `file`, `line_number`, and `line_text`.
2. Implementation: walk the working directory respecting `VEX_WORKDIR` confinement,
   skip binary files, return matches sorted by file path then line number.
3. Add `find_files(name_glob: &str) -> Result<Vec<PathBuf>>` for filename-pattern search.
4. Both methods must respect the existing path-safety guard (lexical + canonical) from
   SEC-01. No paths outside `VEX_WORKDIR` are returned.
5. Register both as tool schemas available to the model alongside existing tools.

---

## Definition of Done

1. `search_content("fn main", None)` returns at least one `SearchMatch` in a repo
   containing `fn main`.
2. Results do not include paths outside the configured working directory.
3. `find_files("*.toml")` returns `Cargo.toml` when called from the repo root.
4. Existing tool operator tests remain green.
5. `cargo test --all-targets` is green.

---

## Anchor Verification

`test_content_search_returns_matched_lines_within_workdir`

```rust
#[test]
fn test_content_search_returns_matched_lines_within_workdir() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("lib.rs");
    std::fs::write(&src, "pub fn greet() -> &'static str { \"hello\" }\n").unwrap();
    let op = ToolOperator::new(dir.path().to_path_buf());
    let matches = op.search_content("greet", None).expect("search failed");
    assert!(!matches.is_empty());
    assert!(matches[0].line_text.contains("greet"));
    assert!(matches[0].file.starts_with(dir.path()));
}
```

**What NOT to do:**
- Do not add network or external process invocations (no `ripgrep` subprocess).
- Do not modify `src/runtime/`, `src/state/`, or `src/api/`.
- Do not bypass the path-safety guard from SEC-01.
- Do not add new `UiUpdate` variants.
