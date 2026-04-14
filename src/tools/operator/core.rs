use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Component, Path, PathBuf};

use super::super::workspace_ignore::WorkspaceIgnore;
use super::{ToolOperator, non_empty_trimmed, path_to_repo_relative_string};

impl ToolOperator {
    pub fn new(working_dir: PathBuf) -> Self {
        let canonical_working_dir =
            fs::canonicalize(&working_dir).unwrap_or_else(|_| working_dir.clone());
        Self {
            working_dir,
            canonical_working_dir,
        }
    }

    pub(in super::super) fn resolve_path(&self, path: &str) -> Result<PathBuf> {
        let path = path.trim();
        if path.is_empty() {
            bail!("Path cannot be empty");
        }

        if path.starts_with('/') || path.contains('\\') {
            bail!("Security error: absolute or platform-specific path not allowed: {path}");
        }

        let relative_path = Path::new(path);
        for component in relative_path.components() {
            if matches!(component, Component::ParentDir) {
                bail!("Security error: path traversal detected: {path}");
            }
        }

        let requested = self.working_dir.join(relative_path);
        let normalized = self.normalize_path(&requested);
        self.ensure_path_is_within_workspace(&normalized)?;

        Ok(normalized)
    }

    pub(in super::super) fn ensure_path_is_within_workspace(&self, path: &Path) -> Result<()> {
        let guard_path = if path.exists() {
            path.to_path_buf()
        } else {
            self.nearest_existing_ancestor(path)
                .context("Security error: could not find an existing parent path")?
                .to_path_buf()
        };

        let canonical_guard = fs::canonicalize(&guard_path)
            .with_context(|| format!("Failed to canonicalize {}", guard_path.display()))?;
        if !canonical_guard.starts_with(&self.canonical_working_dir) {
            bail!(
                "Security error: path escapes working directory via symlink or traversal: {}",
                path.display()
            );
        }
        Ok(())
    }

    fn nearest_existing_ancestor<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        let mut current = path;
        while !current.exists() {
            current = current.parent()?;
        }
        Some(current)
    }

    fn normalize_path(&self, path: &Path) -> PathBuf {
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::Normal(seg) => out.push(seg),
                Component::ParentDir => {
                    if out.components().count() > self.working_dir.components().count() {
                        out.pop();
                    }
                }
                Component::RootDir => out.push(component.as_os_str()),
                Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            }
        }
        out
    }

    pub(in super::super) fn resolve_optional_path(&self, path: Option<&str>) -> Result<PathBuf> {
        match path.and_then(non_empty_trimmed) {
            None => Ok(self.working_dir.clone()),
            Some(".") => Ok(self.working_dir.clone()),
            Some(value) => self.resolve_path(value),
        }
    }

    pub fn to_workspace_relative_display(&self, path: &Path) -> String {
        path.strip_prefix(&self.working_dir)
            .map(path_to_repo_relative_string)
            .unwrap_or_else(|_| path_to_repo_relative_string(path))
    }

    pub fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    pub(super) fn walk_workspace_files(&self, root: &Path) -> Result<Vec<PathBuf>> {
        let ignore = WorkspaceIgnore::load(&self.working_dir);
        self.walk_workspace_files_ignoring(root, &ignore)
    }

    pub(in super::super) fn walk_workspace_files_ignoring(
        &self,
        root: &Path,
        ignore: &WorkspaceIgnore,
    ) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        let mut stack = vec![root.to_path_buf()];

        while let Some(path) = stack.pop() {
            if self.ensure_path_is_within_workspace(&path).is_err() {
                continue;
            }

            let is_dir = path.is_dir();
            let rel = path
                .strip_prefix(&self.working_dir)
                .map(path_to_repo_relative_string)
                .unwrap_or_default();
            if !rel.is_empty() && ignore.is_ignored(&rel, is_dir) {
                continue;
            }

            if is_dir {
                let mut entries_in_dir: Vec<_> = fs::read_dir(&path)
                    .with_context(|| format!("Failed to read directory {}", path.display()))?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .with_context(|| format!("Failed to list entries in {}", path.display()))?;
                entries_in_dir.sort_by_key(|entry| entry.path());
                for entry in entries_in_dir {
                    let entry_path = entry.path();
                    if self.ensure_path_is_within_workspace(&entry_path).is_ok() {
                        stack.push(entry_path);
                    }
                }
                continue;
            }

            files.push(path);
        }

        Ok(files)
    }
}
