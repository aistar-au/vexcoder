mod core;
mod file_ops;
mod git_ops;
pub(crate) mod policy;
mod search;

use std::path::{Path, PathBuf};

#[cfg(test)]
use crate::tools::workspace_explore::glob_matches;

#[derive(Clone)]
pub struct ToolOperator {
    working_dir: PathBuf,
    canonical_working_dir: PathBuf,
}

pub struct PendingPatch {
    pub diff: String,
    pub new_content: String,
    pub path: PathBuf,
}

pub enum WriteFileOutcome {
    Written,
    Pending(PendingPatch),
}

pub struct SearchMatch {
    pub file: PathBuf,
    pub line_number: usize,
    pub line_text: String,
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn path_to_repo_relative_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_empty_path_rejected() {
        let temp = TempDir::new().expect("temp dir");
        let executor = ToolOperator::new(temp.path().to_path_buf());

        let err = executor
            .read_file("   ")
            .expect_err("empty path should fail");
        assert!(err.to_string().contains("Path cannot be empty"));
    }

    #[test]
    fn test_edit_file_rejects_directory_target() {
        let temp = TempDir::new().expect("temp dir");
        let executor = ToolOperator::new(temp.path().to_path_buf());

        let err = executor
            .edit_file(".", "old", "new")
            .expect_err("directory path should fail");
        assert!(
            err.to_string()
                .contains("edit_file expected a file path, got a directory")
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_search_literal_skips_symlink_escape_paths() {
        
        
        use std::os::unix::fs::symlink;

        let workspace = TempDir::new().expect("workspace");
        let outside = TempDir::new().expect("outside");
        let executor = ToolOperator::new(workspace.path().to_path_buf());

        fs::write(outside.path().join("secret.txt"), "top secret\n").expect("seed outside");
        symlink(outside.path(), workspace.path().join("out")).expect("create symlink");

        let result = executor
            .search_literal("secret", workspace.path(), 20)
            .expect("literal search should succeed");
        assert_eq!(result, "No matches found.");
    }

    #[test]
    fn test_write_existing_file_requires_approval_not_direct_write() {
        let dir = tempfile::tempdir().unwrap();
        let op = ToolOperator::new(dir.path().to_path_buf());

        
        op.write_file("target.rs", "fn old() {}")
            .expect("write failed");
        assert_eq!(
            fs::read_to_string(dir.path().join("target.rs")).unwrap(),
            "fn old() {}"
        );

        
        let pending = op
            .propose_patch("target.rs", "fn old() {}", "fn new() {}")
            .expect("propose failed");

        
        assert_eq!(
            fs::read_to_string(dir.path().join("target.rs")).unwrap(),
            "fn old() {}"
        );

        
        op.apply_patch(pending).expect("apply failed");
        assert!(
            fs::read_to_string(dir.path().join("target.rs"))
                .unwrap()
                .contains("fn new()")
        );
    }

    #[test]
    fn test_content_search_returns_matched_lines_within_workdir() {
        let dir = tempfile::tempdir().unwrap();
        let op = ToolOperator::new(dir.path().to_path_buf());
        fs::write(
            dir.path().join("lib.rs"),
            "pub fn greet() -> &'static str { \"hello\" }\n",
        )
        .unwrap();
        let matches = op.search_content("greet", None).expect("search failed");
        assert!(!matches.is_empty());
        assert!(matches[0].line_text.contains("greet"));
        assert!(matches[0].file.starts_with(dir.path()));
    }

    #[test]
    fn test_read_file_if_exists_returns_none_for_missing_path() {
        let dir = tempfile::tempdir().unwrap();
        let op = ToolOperator::new(dir.path().to_path_buf());
        let content = op
            .read_file_if_exists("missing.rs")
            .expect("missing file check should succeed");
        assert!(content.is_none());
    }

    #[test]
    fn test_glob_double_star_matches_root_files() {
        assert!(
            glob_matches("**", "Cargo.toml"),
            "** must match root-level files"
        );
        assert!(
            glob_matches("**", "src/main.rs"),
            "** must match nested files"
        );
        assert!(
            glob_matches("**", "adr/ADR-README.md"),
            "** must match nested files with dashes"
        );
    }

    #[test]
    fn test_glob_double_star_slash_star_misses_root_files() {
        assert!(
            !glob_matches("**/*", "Cargo.toml"),
            "**/* must NOT match root-level files (requires /)"
        );
        assert!(
            glob_matches("**/*", "src/main.rs"),
            "**/* must match nested files"
        );
    }

    #[test]
    fn test_find_files_double_star_includes_root_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        let op = ToolOperator::new(dir.path().to_path_buf());

        let files = op.find_files("**").expect("find_files");
        let names: Vec<String> = files
            .iter()
            .map(|p| op.to_workspace_relative_display(p))
            .collect();

        assert!(
            names.iter().any(|n| n == "Cargo.toml"),
            "find_files(**) must include root-level Cargo.toml, got: {names:?}"
        );
        assert!(
            names.iter().any(|n| n == "src/main.rs"),
            "find_files(**) must include nested src/main.rs, got: {names:?}"
        );
    }

    #[test]
    fn test_find_files_bare_glob_matches_nested_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn lib() {}").unwrap();
        let op = ToolOperator::new(dir.path().to_path_buf());

        let files = op.find_files("*.rs").expect("find_files");
        let names: Vec<String> = files
            .iter()
            .map(|path| op.to_workspace_relative_display(path))
            .collect();

        assert!(names.iter().any(|name| name == "src/main.rs"));
        assert!(names.iter().any(|name| name == "src/lib.rs"));
    }

    #[test]
    fn test_search_content_bare_glob_matches_nested_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "needle\n").unwrap();
        fs::write(dir.path().join("src/data.txt"), "needle\n").unwrap();
        let op = ToolOperator::new(dir.path().to_path_buf());

        let matches = op
            .search_content("needle", Some("*.rs"))
            .expect("search_content");
        let names: Vec<String> = matches
            .iter()
            .map(|m| op.to_workspace_relative_display(&m.file))
            .collect();

        assert_eq!(names, vec!["src/main.rs".to_string()]);
    }
}
