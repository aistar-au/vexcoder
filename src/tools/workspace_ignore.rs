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
    pub fn is_ignored(&self, relative_path: &str) -> bool {
        // Try as-is, then try with a trailing slash (for directory patterns).
        // The `ignore` crate requires `is_dir = true` for directory-only rules
        // to match, but callers don't always know the entry type.
        self.matcher
            .matched_path_or_any_parents(relative_path, false)
            .is_ignore()
            || self
                .matcher
                .matched_path_or_any_parents(relative_path, true)
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

    #[test]
    fn test_wildcard_star_extension_via_workspace() {
        let dir = workspace_with("*.log\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored("error.log"));
        assert!(ign.is_ignored("dir/error.log")); // gitignore *.log matches in subdirs
        assert!(!ign.is_ignored("error.rs"));
    }

    #[test]
    fn test_wildcard_double_star_via_workspace() {
        let dir = workspace_with("**/*.log\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored("dir/sub/error.log"));
        assert!(ign.is_ignored("error.log"));
        assert!(!ign.is_ignored("error.rs"));
    }

    #[test]
    fn test_wildcard_question_mark_via_workspace() {
        let dir = workspace_with("file?.txt\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored("file1.txt"));
        assert!(!ign.is_ignored("file12.txt"));
    }

    #[test]
    fn test_character_class_literal_via_workspace() {
        let dir = workspace_with(".session[a]rea\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored(".sessionarea"));
        assert!(!ign.is_ignored(".sessionbrea"));
    }

    #[test]
    fn test_character_class_range_via_workspace() {
        let dir = workspace_with("file[0-9].txt\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored("file7.txt"));
        assert!(!ign.is_ignored("filex.txt"));
    }

    #[test]
    fn test_workspace_ignore_loads_extension_pattern() {
        let dir = workspace_with("*.log\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored("error.log"));
        assert!(ign.is_ignored("logs/error.log"));
        assert!(!ign.is_ignored("main.rs"));
    }

    #[test]
    fn test_workspace_ignore_directory_pattern() {
        let dir = workspace_with("target/\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored("target"));
        assert!(ign.is_ignored("target/debug/vex"));
    }

    #[test]
    fn test_workspace_ignore_negation() {
        let dir = workspace_with("*.log\n!keep.log\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored("discard.log"));
        assert!(!ign.is_ignored("keep.log"));
    }

    #[test]
    fn test_workspace_ignore_comment_and_blank() {
        let dir = workspace_with("# comment\n\n*.tmp\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored("scratch.tmp"));
        assert!(!ign.is_ignored("Cargo.toml"));
    }

    #[test]
    fn test_workspace_ignore_no_gitignore_never_ignores() {
        let dir = tempfile::tempdir().unwrap();
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(!ign.is_ignored("src/main.rs"));
        assert!(!ign.is_ignored("anything"));
    }

    #[test]
    fn test_workspace_ignore_rooted_pattern() {
        let dir = workspace_with("/CHANGELOG.md\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored("CHANGELOG.md"));
        assert!(!ign.is_ignored("docs/CHANGELOG.md"));
    }

    #[test]
    fn test_workspace_ignore_character_class_pattern() {
        let dir = workspace_with(".session[a]rea/\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored(".sessionarea"));
        assert!(ign.is_ignored(".sessionarea/settings.json"));
        assert!(!ign.is_ignored(".sessionbrea/settings.json"));
    }
}
