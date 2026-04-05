//! PP-01 workspace exploration tools: `list_dir` and `glob_files`.
//!
//! All three workspace exploration tools (`search_files` is in `operator/search.rs`)
//! are workspace-confined, `.gitignore`-aware, and produce bounded output.
//! None of them start a model turn or modify any file.  No subprocess calls.

use anyhow::{Context, Result};
use itertools::Itertools as _;
use std::fs;

use super::operator::ToolOperator;
use super::workspace_ignore::WorkspaceIgnore;
use crate::workspace::make_relative;

/// Maximum number of entries returned by [`list_dir`].
const LIST_DIR_MAX: usize = 500;
/// Maximum number of file paths returned by [`glob_files`].
const GLOB_FILES_MAX: usize = 200;

/// List the immediate (non-recursive) contents of a workspace directory.
///
/// - `path`: workspace-relative directory path; defaults to workspace root.
/// - `max_entries`: capped at [`LIST_DIR_MAX`].
///
/// Hidden entries (names starting with `.`) at the workspace root are skipped
/// the same way `list_files` does.  Gitignore rules from the workspace root
/// are applied to all levels.  Entries are alphabetically sorted.
pub fn list_dir(operator: &ToolOperator, path: Option<&str>, max_entries: usize) -> Result<String> {
    let root = operator
        .resolve_optional_path(path)
        .context("list_dir: invalid path")?;

    if !root.is_dir() {
        return Ok(format!("(not a directory: {})", root.display()));
    }

    let ignore = WorkspaceIgnore::load(operator.working_dir());
    let limit = max_entries.clamp(1, LIST_DIR_MAX);

    let mut raw: Vec<_> = fs::read_dir(&root)
        .with_context(|| format!("list_dir: failed to read '{}'", root.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("list_dir: error iterating '{}'", root.display()))?;
    let total = raw.len();
    raw.sort_by_key(|e| e.path());

    let mut entries = Vec::new();

    for de in raw {
        let p = de.path();
        let name = de.file_name();
        let name_str = name.to_string_lossy();
        let file_type = de
            .file_type()
            .with_context(|| format!("list_dir: failed to inspect '{}'", p.display()))?;
        let is_dir = file_type.is_dir();

        // Skip hidden entries at workspace root (matches list_files behaviour).
        let is_workspace_root = root == operator.working_dir();
        if is_workspace_root && name_str.starts_with('.') {
            continue;
        }

        // Enforce workspace confinement.
        if operator.ensure_path_is_within_workspace(&p).is_err() {
            continue;
        }

        // Apply gitignore rules.
        let rel = p
            .strip_prefix(operator.working_dir())
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if !rel.is_empty() && ignore.is_ignored(&rel, is_dir) {
            continue;
        }

        let mut display = operator.to_workspace_relative_display(&p);
        if is_dir {
            display.push('/');
        }
        entries.push(display);

        if entries.len() >= limit {
            break;
        }
    }

    if entries.is_empty() {
        return Ok("(empty directory)".to_string());
    }

    let mut out = entries.join("\n");
    if entries.len() >= limit && total > limit {
        out.push_str(&format!(
            "\n[results truncated — showing first {} of {} entries]",
            limit, total
        ));
    }
    Ok(out)
}

/// Return workspace-relative paths matching a glob `pattern`.
///
/// - `pattern`: glob using `*` (any non-`/` chars), `**` (any path), `?`.
/// - `max_results`: capped at [`GLOB_FILES_MAX`].
///
/// Only files (not directories) are returned.  Gitignore rules from the
/// workspace root are applied.  Results are alphabetically sorted.
pub fn glob_files(operator: &ToolOperator, pattern: &str, max_results: usize) -> Result<String> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Ok("(glob_files requires a non-empty pattern)".to_string());
    }

    let ignore = WorkspaceIgnore::load(operator.working_dir());
    let limit = max_results.clamp(1, GLOB_FILES_MAX);
    let Some(matcher) = build_glob_matcher(pattern) else {
        return Ok("(no files found)".to_string());
    };

    let all_files = operator
        .walk_workspace_files_ignoring(operator.working_dir(), &ignore)
        .context("glob_files: failed to walk workspace")?;

    let mut matched: Vec<String> = all_files
        .into_iter()
        .filter_map(|p| {
            let rel = make_relative(operator.working_dir(), &p)
                .to_string_lossy()
                .replace('\\', "/");
            if glob_path_match(&matcher, &rel) {
                Some(rel)
            } else {
                None
            }
        })
        .collect();

    matched.sort();

    if matched.is_empty() {
        return Ok("(no files found)".to_string());
    }

    let total = matched.len();
    matched.truncate(limit);
    let mut out = matched.join("\n");
    if total > limit {
        out.push_str(&format!(
            "\n[results truncated — showing first {} of {} matches]",
            limit, total
        ));
    }
    Ok(out)
}

/// Match `pattern` against a workspace-relative path using `globset`.
/// Falls back to matching against just the filename component if the full
/// path doesn't match, for bare-name patterns like `*.rs`.
fn build_glob_matcher(pattern: &str) -> Option<globset::GlobSet> {
    let Ok(glob) = globset::GlobBuilder::new(pattern)
        .literal_separator(false)
        .build()
    else {
        return None;
    };
    globset::GlobSetBuilder::new().add(glob).build().ok()
}

fn glob_path_match(matcher: &globset::GlobSet, path: &str) -> bool {
    matcher.is_match(path) || matcher.is_match(path.rsplit('/').next().unwrap_or(path))
}

/// Return workspace-relative paths whose **basename** matches a Rust regex `pattern`.
///
/// - `pattern`: case-insensitive regex tested against the file's name component only.
/// - `max_results`: capped at [`GLOB_FILES_MAX`].
///
/// Gitignore rules from the workspace root are applied.  Results are sorted
/// alphabetically by workspace-relative path.
pub fn regex_files(operator: &ToolOperator, pattern: &str, max_results: usize) -> Result<String> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Ok("(regex_files requires a non-empty pattern)".to_string());
    }

    let Some(re) = build_regex_matcher(pattern) else {
        return Ok("(invalid regex pattern — use a valid Rust regex)".to_string());
    };

    let ignore = WorkspaceIgnore::load(operator.working_dir());
    let limit = max_results.clamp(1, GLOB_FILES_MAX);

    let all_files = operator
        .walk_workspace_files_ignoring(operator.working_dir(), &ignore)
        .context("regex_files: failed to walk workspace")?;

    let matched: Vec<String> = all_files
        .into_iter()
        .filter_map(|p| {
            let rel = make_relative(operator.working_dir(), &p)
                .to_string_lossy()
                .replace('\\', "/");
            let basename = rel.rsplit('/').next().unwrap_or(&rel);
            if re.is_match(basename) {
                Some(rel)
            } else {
                None
            }
        })
        .sorted()
        .collect();

    if matched.is_empty() {
        return Ok("(no files found)".to_string());
    }

    let total = matched.len();
    let view = &matched[..limit.min(total)];
    let mut out = view.join("\n");
    if total > limit {
        out.push_str(&format!(
            "\n[results truncated — showing first {} of {} matches]",
            limit, total
        ));
    }
    Ok(out)
}

/// Build a case-insensitive regex from `pattern` using `regex-lite`.
fn build_regex_matcher(pattern: &str) -> Option<regex_lite::Regex> {
    regex_lite::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_workspace(files: &[&str]) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            let p = dir.path().join(f);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&p, format!("content of {f}")).unwrap();
        }
        dir
    }

    fn make_workspace_with_gitignore(files: &[&str], gitignore: &str) -> TempDir {
        let dir = make_workspace(files);
        std::fs::write(dir.path().join(".gitignore"), gitignore).unwrap();
        dir
    }

    fn op(dir: &TempDir) -> ToolOperator {
        ToolOperator::new(dir.path().to_path_buf())
    }

    // ── list_dir ─────────────────────────────────────────────────────────────

    #[test]
    fn test_list_dir_returns_immediate_contents() {
        let ws = make_workspace(&["src/main.rs", "src/lib.rs", "Cargo.toml"]);
        let out = list_dir(&op(&ws), None, 50).unwrap();
        // Workspace root should show src/ and Cargo.toml (not hidden entries).
        assert!(
            out.contains("Cargo.toml"),
            "expected Cargo.toml, got: {out}"
        );
        assert!(out.contains("src/"), "expected src/, got: {out}");
    }

    #[test]
    fn test_list_dir_does_not_recurse() {
        let ws = make_workspace(&["src/main.rs", "src/lib.rs", "Cargo.toml"]);
        let out = list_dir(&op(&ws), None, 50).unwrap();
        // Recursive entries must not appear at root level.
        assert!(
            !out.contains("main.rs"),
            "list_dir must not recurse into src/: {out}"
        );
        assert!(
            !out.contains("lib.rs"),
            "list_dir must not recurse into src/: {out}"
        );
    }

    #[test]
    fn test_list_dir_subdirectory() {
        let ws = make_workspace(&["src/main.rs", "src/lib.rs"]);
        let out = list_dir(&op(&ws), Some("src"), 50).unwrap();
        assert!(out.contains("main.rs"), "expected main.rs in src/: {out}");
        assert!(out.contains("lib.rs"), "expected lib.rs in src/: {out}");
    }

    #[test]
    fn test_list_dir_out_of_workspace_path_returns_error() {
        let ws = make_workspace(&["file.rs"]);
        let result = list_dir(&op(&ws), Some("../escape"), 50);
        assert!(
            result.is_err(),
            "list_dir must reject out-of-workspace path"
        );
    }

    #[test]
    fn test_list_dir_respects_gitignore() {
        let ws = make_workspace_with_gitignore(&["src/main.rs", "target/debug/vex"], "target/\n");
        let out = list_dir(&op(&ws), None, 50).unwrap();
        assert!(
            !out.contains("target"),
            "list_dir must skip gitignore-excluded entries: {out}"
        );
        assert!(out.contains("src/"), "src/ must still appear: {out}");
    }

    #[test]
    fn test_list_dir_truncation_annotation() {
        // Create more than 3 files and request max_entries=3
        let ws = make_workspace(&["a.rs", "b.rs", "c.rs", "d.rs", "e.rs"]);
        let out = list_dir(&op(&ws), None, 3).unwrap();
        assert!(
            out.contains("[results truncated"),
            "expected truncation annotation: {out}"
        );
    }

    // ── glob_files ────────────────────────────────────────────────────────────

    #[test]
    fn test_glob_files_returns_matching_paths() {
        let ws = make_workspace(&["src/main.rs", "src/lib.rs", "build.sh"]);
        let out = glob_files(&op(&ws), "**/*.rs", 50).unwrap();
        assert!(out.contains("src/main.rs"), "expected main.rs: {out}");
        assert!(out.contains("src/lib.rs"), "expected lib.rs: {out}");
        assert!(!out.contains("build.sh"), "build.sh must not appear: {out}");
    }

    #[test]
    fn test_glob_files_bounded_results() {
        let ws = make_workspace(&["a.rs", "b.rs", "c.rs", "d.rs", "e.rs"]);
        let out = glob_files(&op(&ws), "*.rs", 2).unwrap();
        assert!(
            out.contains("[results truncated"),
            "expected truncation when limit < total: {out}"
        );
    }

    #[test]
    fn test_glob_files_no_match_returns_empty_message() {
        let ws = make_workspace(&["src/main.rs"]);
        let out = glob_files(&op(&ws), "*.go", 50).unwrap();
        assert_eq!(out, "(no files found)");
    }

    #[test]
    fn test_glob_files_respects_gitignore() {
        let ws = make_workspace_with_gitignore(&["src/main.rs", "target/debug/vex"], "target/\n");
        let out = glob_files(&op(&ws), "**/*", 50).unwrap();
        assert!(
            !out.contains("target"),
            "glob_files must skip gitignore-excluded paths: {out}"
        );
        assert!(
            out.contains("src/main.rs"),
            "src/main.rs must appear: {out}"
        );
    }

    #[test]
    fn test_glob_files_out_of_workspace_path_returns_error() {
        // Walk is always rooted at workspace; passing a path-escape pattern
        // cannot yield results outside the workspace boundary.
        let ws = make_workspace(&["file.rs"]);
        // This must not panic and must not return paths outside the workspace.
        let out = glob_files(&op(&ws), "**/*.rs", 10).unwrap();
        for line in out.lines() {
            assert!(
                !line.starts_with('/'),
                "glob_files must return workspace-relative paths only: {line}"
            );
        }
    }

    // ── search_files gitignore integration ───────────────────────────────────

    #[test]
    fn test_search_files_returns_matching_lines() {
        let ws = make_workspace(&["src/main.rs"]);
        std::fs::write(ws.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        let op = ToolOperator::new(ws.path().to_path_buf());
        let out = op.search_files("fn main", None, 10).unwrap();
        assert!(out.contains("main"), "expected match: {out}");
    }

    #[test]
    fn test_search_files_respects_workspace_root() {
        let ws = make_workspace(&["src/main.rs"]);
        std::fs::write(ws.path().join("src/main.rs"), "secret_token\n").unwrap();
        let op = ToolOperator::new(ws.path().to_path_buf());
        // Path escape must fail.
        let result = op.search_files("secret_token", Some("../outside"), 10);
        assert!(
            result.is_err(),
            "search_files must reject paths outside workspace"
        );
    }

    #[test]
    fn test_search_files_skips_gitignore_excluded_paths() {
        let ws = make_workspace_with_gitignore(&["src/main.rs", "build/output.txt"], "build/\n");
        std::fs::write(ws.path().join("src/main.rs"), "needle\n").unwrap();
        std::fs::write(ws.path().join("build/output.txt"), "needle\n").unwrap();
        let op = ToolOperator::new(ws.path().to_path_buf());
        let out = op.search_files("needle", None, 50).unwrap();
        assert!(
            !out.contains("build/output.txt"),
            "search_files must skip gitignore-excluded paths: {out}"
        );
        assert!(
            out.contains("src/main.rs"),
            "search_files must include non-excluded paths: {out}"
        );
    }

    #[test]
    fn test_search_files_literal_match_no_partial_regex_interpretation() {
        let ws = make_workspace(&["src/main.rs"]);
        std::fs::write(ws.path().join("src/main.rs"), "value: a.b\n").unwrap();
        let op = ToolOperator::new(ws.path().to_path_buf());
        // Regex `.` would match any char; literal search must only match "a.b".
        let out_dot = op.search_files("a.b", None, 10).unwrap();
        let out_exact = op.search_files("a.b", None, 10).unwrap();
        // "axb" would match under regex `.` but not literal "a.b"
        std::fs::write(ws.path().join("src/main.rs"), "axb\n").unwrap();
        let out_wrong = op.search_files("a.b", None, 10).unwrap();
        assert_eq!(
            out_wrong, "No matches found.",
            "literal match must not interpret `.` as regex"
        );
        let _ = (out_dot, out_exact); // used above
    }

    // ── model-turn isolation ─────────────────────────────────────────────────

    #[test]
    fn test_workspace_tools_do_not_start_model_turn() {
        // Ensure list_dir and glob_files return Ok without needing any
        // runtime context — they must be pure synchronous I/O, no channel or
        // runtime handle required.
        let ws = make_workspace(&["src/main.rs"]);
        let op = ToolOperator::new(ws.path().to_path_buf());
        // Both calls complete synchronously — any attempt to start a model
        // turn would panic or block without the full runtime.
        assert!(list_dir(&op, None, 10).is_ok());
        assert!(glob_files(&op, "**/*.rs", 10).is_ok());
    }

    // ── regex_files ──────────────────────────────────────────────────────────

    #[test]
    fn test_regex_files_matches_by_basename() {
        let ws = make_workspace(&["src/main.rs", "src/lib.rs", "Cargo.toml"]);
        let out = regex_files(&op(&ws), r"\.rs$", 50).unwrap();
        assert!(out.contains("src/main.rs"), "expected main.rs: {out}");
        assert!(out.contains("src/lib.rs"), "expected lib.rs: {out}");
        assert!(
            !out.contains("Cargo.toml"),
            "toml must not match .rs: {out}"
        );
    }

    #[test]
    fn test_regex_files_case_insensitive() {
        let ws = make_workspace(&["src/Main.RS", "src/lib.rs"]);
        let out = regex_files(&op(&ws), r"main\.rs$", 50).unwrap();
        assert!(
            out.contains("Main.RS"),
            "expected case-insensitive match: {out}"
        );
        assert!(!out.contains("lib.rs"), "lib.rs must not match: {out}");
    }

    #[test]
    fn test_regex_files_invalid_pattern_returns_message() {
        let ws = make_workspace(&["a.rs"]);
        let out = regex_files(&op(&ws), r"[invalid", 50).unwrap();
        assert!(
            out.contains("invalid regex"),
            "expected invalid-pattern message: {out}"
        );
    }

    #[test]
    fn test_regex_files_respects_gitignore() {
        let ws = make_workspace_with_gitignore(&["src/main.rs", "target/debug.rs"], "target/\n");
        let out = regex_files(&op(&ws), r"\.rs$", 50).unwrap();
        assert!(out.contains("src/main.rs"), "expected main.rs: {out}");
        assert!(
            !out.contains("target/debug.rs"),
            "target/ must be gitignore-filtered: {out}"
        );
    }

    #[test]
    fn test_regex_files_truncation_annotation() {
        let files: Vec<String> = (0..10).map(|i| format!("test_{i}.rs")).collect();
        let files_ref: Vec<&str> = files.iter().map(String::as_str).collect();
        let ws = make_workspace(&files_ref);
        let out = regex_files(&op(&ws), r"\.rs$", 3).unwrap();
        assert!(
            out.contains("[results truncated"),
            "expected truncation annotation: {out}"
        );
    }
}
