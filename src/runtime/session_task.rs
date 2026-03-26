use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub type SessionTaskId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionTaskStatus {
    Pending,
    Running,
    Blocked,
    Failed,
    Cancelled,
    Completed,
}

impl SessionTaskStatus {
    pub fn is_live(&self) -> bool {
        matches!(
            self,
            SessionTaskStatus::Pending | SessionTaskStatus::Running | SessionTaskStatus::Blocked
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTask {
    pub id: SessionTaskId,
    pub parent_task_id: String,
    pub agent_id: String,
    pub prompt: String,
    pub lifecycle_state: SessionTaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heartbeat: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_summary: Option<String>,
}

impl SessionTask {
    pub fn new(
        parent_task_id: impl Into<String>,
        agent_id: impl Into<String>,
        prompt: impl Into<String>,
        worktree_path: Option<PathBuf>,
    ) -> Self {
        let parent_task_id = parent_task_id.into();
        let agent_id = agent_id.into();
        let now = now_millis();
        let id = format!("{}-{}-{}", parent_task_id, agent_id, now);

        Self {
            id,
            parent_task_id,
            agent_id,
            prompt: prompt.into(),
            lifecycle_state: SessionTaskStatus::Pending,
            worktree_path,
            started_at: None,
            updated_at: now,
            last_heartbeat: None,
            handoff_summary: None,
        }
    }

    pub fn transition_to(&mut self, status: SessionTaskStatus) {
        if self.started_at.is_none() && matches!(status, SessionTaskStatus::Running) {
            self.started_at = Some(now_millis());
        }
        self.lifecycle_state = status;
        self.updated_at = now_millis();
    }

    pub fn record_heartbeat(&mut self) {
        let now = now_millis();
        self.last_heartbeat = Some(now);
        self.updated_at = now;
    }

    pub fn set_handoff_summary(&mut self, summary: impl Into<String>) {
        self.handoff_summary = Some(summary.into());
        self.updated_at = now_millis();
    }
}

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_task_transitions_record_timestamps() {
        let mut task = SessionTask::new("parent-1", "reviewer", "inspect docs", None);
        assert_eq!(task.lifecycle_state, SessionTaskStatus::Pending);
        assert!(task.started_at.is_none());

        task.transition_to(SessionTaskStatus::Running);
        assert_eq!(task.lifecycle_state, SessionTaskStatus::Running);
        assert!(task.started_at.is_some());

        task.record_heartbeat();
        assert!(task.last_heartbeat.is_some());

        task.set_handoff_summary("summary");
        assert_eq!(task.handoff_summary.as_deref(), Some("summary"));
    }
}
