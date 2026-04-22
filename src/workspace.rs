use std::path::{Path, PathBuf};

pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut current = start;
    loop {
        if current.join(".git").exists() {
            return Some(dunce::simplified(current).to_path_buf());
        }
        current = current.parent()?;
    }
}

pub fn workspace_root(start: &Path) -> PathBuf {
    find_repo_root(start).unwrap_or_else(|| start.to_path_buf())
}

pub fn resolve_relative_to_workspace(start: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        workspace_root(start).join(path)
    }
}


pub fn make_relative(workspace: &Path, absolute: &Path) -> PathBuf {
    pathdiff::diff_paths(dunce::simplified(absolute), dunce::simplified(workspace))
        .unwrap_or_else(|| absolute.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_root_prefers_git_ancestor_for_subdir() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let nested = root.join("src/nested");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(workspace_root(&nested), root);
    }

    #[test]
    fn test_workspace_root_falls_back_to_current_dir_outside_repo() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(workspace_root(temp.path()), temp.path());
    }
}
