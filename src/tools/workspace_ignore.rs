use std::path::Path;

/// A set of `.gitignore` patterns loaded from the workspace root.
///
/// Uses the `ignore` crate's gitignore parser for correct, battle-tested
/// pattern matching (negation, `**`, character classes, anchoring, etc.).
pub struct WorkspaceIgnore {
    matcher: ignore::gitignore::Gitignore,
}

impl Default for WorkspaceIgnore {
    fn default() -> Self {
        let empty = ignore::gitignore::Gitignore::empty();
        Self { matcher: empty }
    }
}

impl WorkspaceIgnore {
    /// Load `.gitignore` from `workspace_root`.  Returns an empty ignore set
    /// on any read or parse error so the walk always continues safely.
    pub fn load(workspace_root: &Path) -> Self {
        let path = workspace_root.join(".gitignore");
        let mut builder = ignore::gitignore::GitignoreBuilder::new(workspace_root);
        if builder.add(&path).is_some() {
            return Self::default();
        }
        match builder.build() {
            Ok(matcher) => Self { matcher },
            Err(_) => Self::default(),
        }
    }

    /// Returns `true` when `relative_path` (forward-slash separated, no
    /// leading slash) should be excluded from a workspace walk.
    pub fn is_ignored(&self, relative_path: &str, is_dir: bool) -> bool {
        self.matcher
            .matched_path_or_any_parents(relative_path, is_dir)
            .is_ignore()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn workspace_with(rules: &str) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), rules).unwrap();
        dir
    }

    fn file_ignored(ignore: &WorkspaceIgnore, relative_path: &str) -> bool {
        ignore.is_ignored(relative_path, false)
    }

    fn dir_ignored(ignore: &WorkspaceIgnore, relative_path: &str) -> bool {
        ignore.is_ignored(relative_path, true)
    }

    #[test]
    fn test_wildcard_star_extension_via_workspace() {
        let dir = workspace_with("*.log\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(file_ignored(&ign, "error.log"));
        assert!(file_ignored(&ign, "dir/error.log")); // gitignore *.log matches in subdirs
        assert!(!file_ignored(&ign, "error.rs"));
    }

    #[test]
    fn test_wildcard_double_star_via_workspace() {
        let dir = workspace_with("**/*.log\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(file_ignored(&ign, "dir/sub/error.log"));
        assert!(file_ignored(&ign, "error.log"));
        assert!(!file_ignored(&ign, "error.rs"));
    }

    #[test]
    fn test_wildcard_question_mark_via_workspace() {
        let dir = workspace_with("file?.txt\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(file_ignored(&ign, "file1.txt"));
        assert!(!file_ignored(&ign, "file12.txt"));
    }

    #[test]
    fn test_character_class_literal_via_workspace() {
        let dir = workspace_with(".session[a]rea\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(file_ignored(&ign, ".sessionarea"));
        assert!(!file_ignored(&ign, ".sessionbrea"));
    }

    #[test]
    fn test_character_class_range_via_workspace() {
        let dir = workspace_with("file[0-9].txt\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(file_ignored(&ign, "file7.txt"));
        assert!(!file_ignored(&ign, "filex.txt"));
    }

    #[test]
    fn test_workspace_ignore_loads_extension_pattern() {
        let dir = workspace_with("*.log\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(file_ignored(&ign, "error.log"));
        assert!(file_ignored(&ign, "logs/error.log"));
        assert!(!file_ignored(&ign, "main.rs"));
    }

    #[test]
    fn test_workspace_ignore_directory_pattern() {
        let dir = workspace_with("target/\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(dir_ignored(&ign, "target"));
        assert!(file_ignored(&ign, "target/debug/vex"));
        assert!(!file_ignored(&ign, "target"));
    }

    #[test]
    fn test_workspace_ignore_negation() {
        let dir = workspace_with("*.log\n!keep.log\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(file_ignored(&ign, "discard.log"));
        assert!(!file_ignored(&ign, "keep.log"));
    }

    #[test]
    fn test_workspace_ignore_comment_and_blank() {
        let dir = workspace_with("# comment\n\n*.tmp\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(file_ignored(&ign, "scratch.tmp"));
        assert!(!file_ignored(&ign, "Cargo.toml"));
    }

    #[test]
    fn test_workspace_ignore_no_gitignore_never_ignores() {
        let dir = tempfile::tempdir().unwrap();
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(!file_ignored(&ign, "src/main.rs"));
        assert!(!file_ignored(&ign, "anything"));
    }

    #[test]
    fn test_workspace_ignore_rooted_pattern() {
        let dir = workspace_with("/CHANGELOG.md\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(file_ignored(&ign, "CHANGELOG.md"));
        assert!(!file_ignored(&ign, "docs/CHANGELOG.md"));
    }

    #[test]
    fn test_workspace_ignore_character_class_pattern() {
        let dir = workspace_with(".session[a]rea/\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(dir_ignored(&ign, ".sessionarea"));
        assert!(file_ignored(&ign, ".sessionarea/settings.json"));
        assert!(!file_ignored(&ign, ".sessionbrea/settings.json"));
    }
}
