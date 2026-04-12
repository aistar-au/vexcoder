//! Lazy-load handle for task-state metadata.
//!
//! `TaskMetadataRef` is forward-declared API consumed by the TUI recent-task
//! list. The methods are not yet wired to a call-site in this batch.

#[cfg(test)]
use anyhow::Result;
#[cfg(test)]
use std::path::PathBuf;

#[cfg(test)]
use super::meta_projection::TaskMetadataHeader;
#[cfg(test)]
use super::{TaskId, TaskState};

/// Opaque reference to task metadata.
///
/// Holds only the projected `TaskMetadataHeader` until the caller
/// explicitly resolves to the full `TaskState` via `.resolve()`.
/// Safe to hold in the UI recent-task list without triggering a full
/// disk read beyond the header projection.
#[cfg(test)]
pub(crate) struct TaskMetadataRef {
    pub id: TaskId,
    dir: PathBuf,
    header: TaskMetadataHeader,
    loaded: bool,
    /// Populated on first `.resolve()` call; None before that.
    state: Option<Box<TaskState>>,
}

#[cfg(test)]
impl TaskMetadataRef {
    pub(crate) fn new(id: TaskId, dir: PathBuf, header: TaskMetadataHeader) -> Self {
        Self {
            id,
            dir,
            header,
            loaded: false,
            state: None,
        }
    }

    /// Resolve to full `TaskState`. Calls `TaskState::load()` on the first
    /// invocation; subsequent calls return the cached result.
    ///
    /// Idempotent: calling `.resolve()` twice does not re-read the file.
    pub(crate) fn resolve(&mut self) -> Result<&TaskState> {
        if !self.loaded {
            let state = TaskState::load(&self.dir, &self.id)?;
            self.state = Some(Box::new(state));
            self.loaded = true;
        }
        Ok(self.state.as_ref().expect("state populated above"))
    }

    /// Access the metadata header without loading the full state.
    pub(crate) fn metadata(&self) -> &TaskMetadataHeader {
        &self.header
    }

    /// Returns true when at least one session sub-task is in a live state
    /// (Pending, Running, or Blocked). Uses only the header — no disk I/O.
    pub(crate) fn has_live_sessions(&self) -> bool {
        self.header
            .session_tasks
            .as_ref()
            .is_some_and(|tasks| tasks.iter().any(|t| t.status.is_live()))
    }

    /// Returns true after `.resolve()` has been called at least once.
    pub(crate) fn is_loaded(&self) -> bool {
        self.loaded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_ref(dir: &TempDir, id: &str) -> TaskMetadataRef {
        // Write a minimal task state so TaskState::load() can succeed.
        let state = TaskState::new(id.to_string());
        state.save(dir.path()).unwrap();

        let header = super::super::meta_projection::TaskMetadataHeader::from_path(
            &dir.path().join(format!("{id}.json")),
        )
        .unwrap();
        TaskMetadataRef::new(id.to_string(), dir.path().to_path_buf(), header)
    }

    #[test]
    fn resolve_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let mut r = make_ref(&dir, "idem");
        assert!(!r.is_loaded());
        let _ = r.resolve().unwrap();
        assert!(r.is_loaded());
        // Second resolve must not re-read the file.
        let _ = r.resolve().unwrap();
        assert!(r.is_loaded());
    }

    #[test]
    fn metadata_accessible_before_resolve() {
        let dir = TempDir::new().unwrap();
        let r = make_ref(&dir, "pre-resolve");
        assert_eq!(r.metadata().id, "pre-resolve");
        assert!(!r.is_loaded());
    }

    #[test]
    fn has_live_sessions_false_when_no_session_tasks() {
        let dir = TempDir::new().unwrap();
        let r = make_ref(&dir, "no-sessions");
        assert!(!r.has_live_sessions());
    }
}
