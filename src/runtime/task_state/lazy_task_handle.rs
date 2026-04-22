

#[cfg(test)]
use anyhow::Result;
#[cfg(test)]
use std::path::PathBuf;

#[cfg(test)]
use super::task_header::TaskStateHeader;
#[cfg(test)]
use super::{TaskId, TaskState};


#[cfg(test)]
pub(crate) struct LazyTaskHandle {
    pub id: TaskId,
    dir: PathBuf,
    header: TaskStateHeader,
    loaded: bool,
    
    state: Option<Box<TaskState>>,
}

#[cfg(test)]
impl LazyTaskHandle {
    pub(crate) fn new(id: TaskId, dir: PathBuf, header: TaskStateHeader) -> Self {
        Self {
            id,
            dir,
            header,
            loaded: false,
            state: None,
        }
    }

    
    pub(crate) fn resolve(&mut self) -> Result<&TaskState> {
        if !self.loaded {
            let state = TaskState::load(&self.dir, &self.id)?;
            self.state = Some(Box::new(state));
            self.loaded = true;
        }
        Ok(self.state.as_ref().expect("state populated above"))
    }

    
    pub(crate) fn header(&self) -> &TaskStateHeader {
        &self.header
    }

    
    pub(crate) fn has_live_sessions(&self) -> bool {
        self.header
            .session_tasks
            .as_ref()
            .is_some_and(|tasks| tasks.iter().any(|t| t.status.is_live()))
    }

    
    pub(crate) fn is_loaded(&self) -> bool {
        self.loaded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_ref(dir: &TempDir, id: &str) -> LazyTaskHandle {
        
        let state = TaskState::new(id.to_string());
        state.save(dir.path()).unwrap();

        let header = super::super::task_header::TaskStateHeader::from_path(
            &dir.path().join(format!("{id}.json")),
        )
        .unwrap();
        LazyTaskHandle::new(id.to_string(), dir.path().to_path_buf(), header)
    }

    #[test]
    fn resolve_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let mut r = make_ref(&dir, "idem");
        assert!(!r.is_loaded());
        let _ = r.resolve().unwrap();
        assert!(r.is_loaded());
        
        let _ = r.resolve().unwrap();
        assert!(r.is_loaded());
    }

    #[test]
    fn header_accessible_before_resolve() {
        let dir = TempDir::new().unwrap();
        let r = make_ref(&dir, "pre-resolve");
        assert_eq!(r.header().id, "pre-resolve");
        assert!(!r.is_loaded());
    }

    #[test]
    fn has_live_sessions_false_when_no_session_tasks() {
        let dir = TempDir::new().unwrap();
        let r = make_ref(&dir, "no-sessions");
        assert!(!r.has_live_sessions());
    }
}
