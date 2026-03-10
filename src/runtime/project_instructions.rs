//! PA-02 -- project instructions injection (ADR-024 Gap 4).
//!
//! Searches for `.vex/AGENTS.md`, `AGENTS.md`, `.vex/PROJECT.md` under
//! `workspace_root` in that priority order. The first file found is read.
//!
//! If the file's estimated token count (byte length / 4) exceeds
//! `token_budget`, `LoadResult::OverBudget` is returned and the caller must
//! emit a warning and skip injection. The file is never truncated.

use std::path::{Path, PathBuf};

/// Priority-ordered candidate paths relative to the workspace root.
const SEARCH_PATHS: &[&str] = &[".vex/AGENTS.md", "AGENTS.md", ".vex/PROJECT.md"];

fn estimate_tokens(content: &str) -> usize {
    (content.len() / 4).max(1)
}

pub struct ProjectInstructions {
    /// Workspace-relative path, for display in the TUI status line.
    pub path: PathBuf,
    /// Raw file content, verified to be within the token budget.
    pub content: String,
}

pub enum LoadResult {
    Loaded(ProjectInstructions),
    OverBudget {
        path: PathBuf,
        estimated_tokens: usize,
    },
    NotFound,
}

/// Search for a project instructions file under `workspace_root`.
///
/// IO errors other than `NotFound` are silently ignored. The file is optional
/// and a permissions error must not abort agent startup.
pub fn load_project_instructions(workspace_root: &Path, token_budget: usize) -> LoadResult {
    for &relative in SEARCH_PATHS {
        let candidate = workspace_root.join(relative);
        let content = match std::fs::read_to_string(&candidate) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => continue,
        };
        let estimated = estimate_tokens(&content);
        let path = PathBuf::from(relative);
        if estimated > token_budget {
            return LoadResult::OverBudget {
                path,
                estimated_tokens: estimated,
            };
        }
        return LoadResult::Loaded(ProjectInstructions { path, content });
    }
    LoadResult::NotFound
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_within_budget_is_loaded() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("AGENTS.md"), "# no unwrap\n").unwrap();
        match load_project_instructions(dir.path(), 4096) {
            LoadResult::Loaded(project_instructions) => {
                assert!(project_instructions.content.contains("no unwrap"));
                assert_eq!(project_instructions.path, PathBuf::from("AGENTS.md"));
            }
            _ => panic!("expected Loaded"),
        }
    }

    #[test]
    fn test_over_budget_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("AGENTS.md"), "x".repeat(4096 * 5)).unwrap();
        assert!(matches!(
            load_project_instructions(dir.path(), 4096),
            LoadResult::OverBudget { .. }
        ));
    }

    #[test]
    fn test_vex_agents_md_takes_priority_over_root_agents_md() {
        let dir = tempfile::tempdir().unwrap();
        let vex_dir = dir.path().join(".vex");
        fs::create_dir(&vex_dir).unwrap();
        fs::write(vex_dir.join("AGENTS.md"), "vex-level").unwrap();
        fs::write(dir.path().join("AGENTS.md"), "root-level").unwrap();
        match load_project_instructions(dir.path(), 4096) {
            LoadResult::Loaded(project_instructions) => {
                assert_eq!(project_instructions.content.trim(), "vex-level")
            }
            _ => panic!("expected Loaded"),
        }
    }

    #[test]
    fn test_not_found_when_no_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            load_project_instructions(dir.path(), 4096),
            LoadResult::NotFound
        ));
    }

    #[test]
    fn test_project_md_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let vex_dir = dir.path().join(".vex");
        fs::create_dir(&vex_dir).unwrap();
        fs::write(vex_dir.join("PROJECT.md"), "project-content").unwrap();
        match load_project_instructions(dir.path(), 4096) {
            LoadResult::Loaded(project_instructions) => {
                assert_eq!(project_instructions.content.trim(), "project-content")
            }
            _ => panic!("expected Loaded from .vex/PROJECT.md"),
        }
    }
}
