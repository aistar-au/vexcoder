use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::runtime::session_task::now_millis;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeLease {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    pub path: PathBuf,
    pub acquired_at: u64,
}

#[derive(Debug, Clone)]
pub struct WorktreeLeaseManager {
    state_dir: PathBuf,
}

impl WorktreeLeaseManager {
    pub fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
        }
    }

    pub fn lease_root(&self) -> PathBuf {
        self.state_dir.join("worktrees")
    }

    fn metadata_root(&self) -> PathBuf {
        self.state_dir.join("worktree-leases")
    }

    fn metadata_path(&self, task_id: &str) -> PathBuf {
        self.metadata_root().join(format!("{task_id}.json"))
    }

    pub fn lease_for_task(
        &self,
        task_id: &str,
        parent_task_id: Option<&str>,
    ) -> Result<WorktreeLease> {
        std::fs::create_dir_all(self.lease_root())
            .with_context(|| format!("failed to create {}", self.lease_root().display()))?;
        std::fs::create_dir_all(self.metadata_root())
            .with_context(|| format!("failed to create {}", self.metadata_root().display()))?;

        let path = self.lease_root().join(task_id);

        // Attempt real git worktree; fall back to plain directory if not in a
        // git repository (e.g. tests or non-git working directories).
        let git_status = std::process::Command::new("git")
            .args(["worktree", "add", "--detach"])
            .arg(&path)
            .arg("HEAD")
            .current_dir(&self.state_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        match git_status {
            Ok(status) if status.success() => { /* real worktree created */ }
            _ => {
                // Fallback: plain directory (tests, non-git contexts).
                std::fs::create_dir_all(&path)
                    .with_context(|| format!("failed to create {}", path.display()))?;
            }
        }

        let lease = WorktreeLease {
            task_id: task_id.to_string(),
            parent_task_id: parent_task_id.map(ToOwned::to_owned),
            path,
            acquired_at: now_millis(),
        };

        crate::util::write_json_atomic(&self.metadata_path(task_id), &lease, "worktree lease")?;

        Ok(lease)
    }

    pub fn release(&self, task_id: &str) -> Result<()> {
        let metadata_path = self.metadata_path(task_id);
        if metadata_path.exists() {
            let lease = self.load(task_id)?;
            if lease.path.exists() {
                // Try git worktree remove first; fall back to plain removal.
                let removed_via_git = std::process::Command::new("git")
                    .args(["worktree", "remove", "--force"])
                    .arg(&lease.path)
                    .current_dir(&self.state_dir)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if !removed_via_git {
                    std::fs::remove_dir_all(&lease.path)
                        .with_context(|| format!("failed to remove {}", lease.path.display()))?;
                }
            }
            std::fs::remove_file(metadata_path)
                .with_context(|| format!("failed to remove lease metadata for {task_id}"))?;
        }
        Ok(())
    }

    pub fn load(&self, task_id: &str) -> Result<WorktreeLease> {
        let content = std::fs::read_to_string(self.metadata_path(task_id))
            .with_context(|| format!("failed to read lease metadata for {task_id}"))?;
        serde_json::from_str(&content).context("deserialize worktree lease")
    }

    pub fn list(&self) -> Result<Vec<WorktreeLease>> {
        let mut leases = Vec::new();
        let root = self.metadata_root();
        if !root.exists() {
            return Ok(leases);
        }

        for entry in std::fs::read_dir(&root)
            .with_context(|| format!("failed to read {}", root.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            leases.push(serde_json::from_str(&content).context("deserialize worktree lease")?);
        }

        leases.sort_by(|left, right| left.task_id.cmp(&right.task_id));
        Ok(leases)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_manager_persists_and_releases_paths() {
        let temp = tempfile::tempdir().unwrap();
        let manager = WorktreeLeaseManager::new(temp.path());

        let lease = manager
            .lease_for_task("session-1", Some("parent-1"))
            .unwrap();
        assert!(lease.path.exists());

        let loaded = manager.load("session-1").unwrap();
        assert_eq!(loaded.parent_task_id.as_deref(), Some("parent-1"));
        assert_eq!(manager.list().unwrap().len(), 1);

        manager.release("session-1").unwrap();
        assert!(manager.list().unwrap().is_empty());
        assert!(!lease.path.exists());
    }
}
