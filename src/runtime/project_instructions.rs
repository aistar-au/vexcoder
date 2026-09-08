use std::path::{Path, PathBuf};

const CANDIDATE_FILES: &[&str] = &[".vex/AGENTS.md", "AGENTS.md", ".vex/PROJECT.md"];

fn estimate_tokens(content: &str) -> usize {
    crate::runtime::token_count::token_count(content)
}

/// One instruction file considered during the root-to-leaf walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionSource {
    pub path: PathBuf,
    pub included: bool,
    pub estimated_tokens: usize,
}

/// Concatenated instruction text plus the per-file inclusion manifest.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InstructionSet {
    pub content: String,
    pub manifest: Vec<InstructionSource>,
}

impl InstructionSet {
    pub fn included_content(&self) -> Option<String> {
        if self.content.trim().is_empty() {
            None
        } else {
            Some(self.content.clone())
        }
    }

    pub fn display_path(&self) -> Option<String> {
        let included: Vec<String> = self
            .manifest
            .iter()
            .filter(|source| source.included)
            .map(|source| source.path.to_string_lossy().into_owned())
            .collect();
        if included.is_empty() {
            None
        } else {
            Some(included.join("; "))
        }
    }

    pub fn emit_skip_warnings(&self, token_budget: usize) {
        for source in &self.manifest {
            if source.included {
                continue;
            }
            eprintln!(
                "[project instructions] {} skipped: estimated {} tokens exceeds budget of {}",
                source.path.display(),
                source.estimated_tokens,
                token_budget,
            );
        }
    }
}

/// Walk from `repo_root` to `cwd`. At each directory, load the first existing
/// candidate name. Over-budget files are recorded and skipped; the walk continues.
pub fn load_hierarchical_instructions(
    repo_root: &Path,
    cwd: &Path,
    token_budget: usize,
) -> InstructionSet {
    let mut sections = Vec::new();
    let mut manifest = Vec::new();
    let mut remaining = token_budget;

    for dir in directories_root_to_leaf(repo_root, cwd) {
        let Some((path, content)) = first_existing(&dir, CANDIDATE_FILES) else {
            continue;
        };
        let estimated = estimate_tokens(&content);
        let included = estimated <= remaining;
        if included {
            remaining = remaining.saturating_sub(estimated);
            sections.push(content);
        }
        let relative = path.strip_prefix(repo_root).unwrap_or(&path).to_path_buf();
        manifest.push(InstructionSource {
            path: relative,
            included,
            estimated_tokens: estimated,
        });
    }

    InstructionSet {
        content: sections.join("\n\n"),
        manifest,
    }
}

/// Load instructions treating `workspace_root` as both repo root and cwd.
/// Nested walks should call [`load_hierarchical_instructions`] instead.
pub fn load_project_instructions(workspace_root: &Path, token_budget: usize) -> InstructionSet {
    load_hierarchical_instructions(workspace_root, workspace_root, token_budget)
}

/// Resolve the git/workspace root for `working_dir` and walk to that cwd.
pub fn load_instructions_for_workspace(working_dir: &Path, token_budget: usize) -> InstructionSet {
    let repo_root = crate::workspace::workspace_root(working_dir);
    load_hierarchical_instructions(&repo_root, working_dir, token_budget)
}

fn directories_root_to_leaf(repo_root: &Path, cwd: &Path) -> Vec<PathBuf> {
    let Ok(relative) = cwd.strip_prefix(repo_root) else {
        return vec![repo_root.to_path_buf()];
    };
    let mut out = vec![repo_root.to_path_buf()];
    let mut acc = repo_root.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(_) => {
                acc = acc.join(component);
                out.push(acc.clone());
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::Prefix(_)
            | std::path::Component::RootDir => break,
        }
    }
    out
}

fn first_existing(dir: &Path, names: &[&str]) -> Option<(PathBuf, String)> {
    names.iter().find_map(|name| {
        let candidate = dir.join(name);
        std::fs::read_to_string(&candidate)
            .ok()
            .map(|content| (candidate, content))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_within_budget_is_loaded() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("AGENTS.md"), "# no unwrap\n").unwrap();
        let set = load_project_instructions(dir.path(), 4096);
        let content = set.included_content().expect("expected Loaded");
        assert!(content.contains("no unwrap"));
        assert_eq!(set.manifest[0].path, PathBuf::from("AGENTS.md"));
        assert!(set.manifest[0].included);
    }

    #[test]
    fn test_over_budget_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("AGENTS.md"),
            "distinct-token-alpha distinct-token-beta distinct-token-gamma",
        )
        .unwrap();
        let set = load_project_instructions(dir.path(), 1);
        assert!(set.included_content().is_none());
        assert_eq!(set.manifest.len(), 1);
        assert!(!set.manifest[0].included);
    }

    #[test]
    fn test_vex_agents_md_takes_priority_over_root_agents_md() {
        let dir = tempfile::tempdir().unwrap();
        let vex_dir = dir.path().join(".vex");
        fs::create_dir(&vex_dir).unwrap();
        fs::write(vex_dir.join("AGENTS.md"), "vex-level").unwrap();
        fs::write(dir.path().join("AGENTS.md"), "root-level").unwrap();
        let set = load_project_instructions(dir.path(), 4096);
        let content = set.included_content().expect("expected Loaded");
        assert_eq!(content.trim(), "vex-level");
        assert_eq!(set.manifest.len(), 1);
    }

    #[test]
    fn test_not_found_when_no_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let set = load_project_instructions(dir.path(), 4096);
        assert!(set.manifest.is_empty());
        assert!(set.included_content().is_none());
    }

    #[test]
    fn test_project_md_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let vex_dir = dir.path().join(".vex");
        fs::create_dir(&vex_dir).unwrap();
        fs::write(vex_dir.join("PROJECT.md"), "project-content").unwrap();
        let set = load_project_instructions(dir.path(), 4096);
        let content = set
            .included_content()
            .expect("expected Loaded from .vex/PROJECT.md");
        assert_eq!(content.trim(), "project-content");
    }

    #[test]
    fn instruction_walk_falls_back_when_higher_file_is_over_budget() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".git")).unwrap();
        let root_body =
            "distinct-token-alpha distinct-token-beta distinct-token-gamma distinct-token-delta";
        fs::write(root.join("AGENTS.md"), root_body).unwrap();
        let nested = root.join("crate").join("inner");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("AGENTS.md"), "leaf-ok").unwrap();

        let leaf_tokens = estimate_tokens("leaf-ok");
        let root_tokens = estimate_tokens(root_body);
        assert!(
            root_tokens > leaf_tokens,
            "root file must exceed the leaf budget"
        );

        let set = load_hierarchical_instructions(root, &nested, leaf_tokens);
        let content = set.included_content().expect("leaf file should load");
        assert!(content.contains("leaf-ok"));
        assert!(!content.contains("distinct-token-alpha"));
        assert_eq!(set.manifest.len(), 2);
        assert!(!set.manifest[0].included, "root file should be skipped");
        assert!(set.manifest[1].included, "leaf file should be included");
        assert_eq!(set.manifest[1].path, PathBuf::from("crate/inner/AGENTS.md"));
    }

    #[test]
    fn instruction_walk_layers_root_before_leaf() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("AGENTS.md"), "root-section").unwrap();
        let nested = root.join("src");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("AGENTS.md"), "leaf-section").unwrap();

        let set = load_hierarchical_instructions(root, &nested, 4096);
        let content = set.included_content().expect("both files should load");
        let root_pos = content.find("root-section").expect("root present");
        let leaf_pos = content.find("leaf-section").expect("leaf present");
        assert!(
            root_pos < leaf_pos,
            "root content must precede leaf content"
        );
        assert_eq!(set.manifest.len(), 2);
        assert!(set.manifest.iter().all(|source| source.included));
    }

    #[test]
    fn directories_root_to_leaf_includes_each_ancestor() {
        let root = PathBuf::from("repo");
        let cwd = PathBuf::from("repo/src/crate");
        let dirs = directories_root_to_leaf(&root, &cwd);
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("repo"),
                PathBuf::from("repo/src"),
                PathBuf::from("repo/src/crate"),
            ]
        );
    }
}
