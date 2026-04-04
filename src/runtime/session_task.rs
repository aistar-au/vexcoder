use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use uuid::Uuid;

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

impl fmt::Display for SessionTaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => f.write_str("pending"),
            Self::Running => f.write_str("running"),
            Self::Blocked => f.write_str("blocked"),
            Self::Failed => f.write_str("failed"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Completed => f.write_str("completed"),
        }
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
        let id = format!(
            "{}-{}-{}",
            parent_task_id,
            agent_id,
            Uuid::new_v4().as_hyphenated()
        );

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

    #[tracing::instrument(skip(self), fields(id = %self.id, from = %self.lifecycle_state, to = %status))]
    pub fn transition_to(&mut self, status: SessionTaskStatus) {
        if self.started_at.is_none() && matches!(status, SessionTaskStatus::Running) {
            self.started_at = Some(now_millis());
        }
        self.lifecycle_state = status;
        self.updated_at = now_millis();
    }

    #[tracing::instrument(skip(self), fields(id = %self.id))]
    pub fn record_heartbeat(&mut self) {
        let now = now_millis();
        self.last_heartbeat = Some(now);
        self.updated_at = now;
    }

    #[tracing::instrument(skip(self, summary), fields(id = %self.id))]
    pub fn set_handoff_summary(&mut self, summary: impl Into<String>) {
        self.handoff_summary = Some(summary.into());
        self.updated_at = now_millis();
    }
}

pub fn now_millis() -> u64 {
    Utc::now().timestamp_millis().try_into().unwrap_or(u64::MAX)
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

    #[test]
    fn session_task_status_display_uses_lowercase_kebab_names() {
        assert_eq!(SessionTaskStatus::Pending.to_string(), "pending");
        assert_eq!(SessionTaskStatus::Running.to_string(), "running");
        assert_eq!(SessionTaskStatus::Blocked.to_string(), "blocked");
        assert_eq!(SessionTaskStatus::Failed.to_string(), "failed");
        assert_eq!(SessionTaskStatus::Cancelled.to_string(), "cancelled");
        assert_eq!(SessionTaskStatus::Completed.to_string(), "completed");
    }

    #[test]
    fn is_live_returns_true_for_active_states_only() {
        assert!(SessionTaskStatus::Pending.is_live());
        assert!(SessionTaskStatus::Running.is_live());
        assert!(SessionTaskStatus::Blocked.is_live());
        assert!(!SessionTaskStatus::Failed.is_live());
        assert!(!SessionTaskStatus::Cancelled.is_live());
        assert!(!SessionTaskStatus::Completed.is_live());
    }
}
