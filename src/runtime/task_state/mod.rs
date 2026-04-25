use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use crate::pulse_evidence::TurnEvidenceState;
use crate::runtime::session_task::{SessionTask, SessionTaskStatus, now_millis};
use crate::runtime::{ApprovalScope, Capability};

pub(crate) mod header_cache;
#[cfg(test)]
pub(crate) mod lazy_task_handle;
pub(crate) mod peer_channel;
mod persist;
pub(crate) mod task_header;

pub use persist::TaskStateFile;

pub type TaskId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    Ready,
    Running,
    AwaitingApproval,
    Cancelling,
    Completed,
    Failed,
    Cancelled,

    MaxTurnsReached,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready => f.write_str("ready"),
            Self::Running => f.write_str("running"),
            Self::AwaitingApproval => f.write_str("awaiting_approval"),
            Self::Cancelling => f.write_str("cancelling"),
            Self::Completed => f.write_str("completed"),
            Self::Failed => f.write_str("failed"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::MaxTurnsReached => f.write_str("max_turns_reached"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandEvidence {
    pub program: String,
    pub exit_code: Option<i32>,
    pub interrupted: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationCheckpoint {
    pub message_count: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterruptedCommand {
    pub program: String,
    pub interrupted_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionNote {
    pub content: String,
    pub created_at_turn: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextCompactionRecord {
    pub turn_index: usize,
    pub messages_before: usize,
    pub messages_after: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheUsageStats {
    pub total_cache_creation_tokens: u64,
    pub total_cache_read_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskState {
    pub id: TaskId,
    pub status: TaskStatus,
    #[serde(default)]
    pub parent_task_id: Option<TaskId>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heartbeat: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_summary: Option<String>,
    pub active_grants: HashMap<Capability, ApprovalScope>,
    pub changed_files: Vec<PathBuf>,
    pub command_history: Vec<CommandEvidence>,
    pub conversation_snapshot: ConversationCheckpoint,
    pub interrupted_sessions: Vec<InterruptedCommand>,
    #[serde(default)]
    pub branch_name: Option<String>,
    #[serde(default)]
    pub instructions_path: Option<String>,
    #[serde(default)]
    pub pulses: Vec<TurnEvidenceState>,
    #[serde(default)]
    pub plan: Option<String>,
    #[serde(default)]
    pub session_notes: Vec<SessionNote>,
    #[serde(default)]
    pub context_compaction: Vec<ContextCompactionRecord>,
    #[serde(default)]
    pub cache_usage: CacheUsageStats,
    #[serde(default, alias = "child_tasks")]
    pub session_tasks: Vec<SessionTask>,
}

impl TaskState {
    pub fn new(id: TaskId) -> Self {
        let now = now_millis();
        Self {
            id,
            status: TaskStatus::Ready,
            parent_task_id: None,
            agent_id: None,
            worktree_path: None,
            started_at: Some(now),
            updated_at: now,
            last_heartbeat: None,
            handoff_summary: None,
            active_grants: HashMap::new(),
            changed_files: Vec::new(),
            command_history: Vec::new(),
            conversation_snapshot: ConversationCheckpoint::default(),
            interrupted_sessions: Vec::new(),
            branch_name: None,
            instructions_path: None,
            pulses: Vec::new(),
            plan: None,
            session_notes: Vec::new(),
            context_compaction: Vec::new(),
            cache_usage: CacheUsageStats::default(),
            session_tasks: Vec::new(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = now_millis();
    }

    pub fn record_heartbeat(&mut self) {
        let now = now_millis();
        self.last_heartbeat = Some(now);
        self.updated_at = now;
    }

    pub fn add_session_task(&mut self, session_task: SessionTask) {
        self.session_tasks.push(session_task);
        self.touch();
    }

    pub fn session_task(&self, id: &str) -> Option<&SessionTask> {
        self.session_tasks.iter().find(|task| task.id == id)
    }

    pub fn session_task_mut(&mut self, id: &str) -> Option<&mut SessionTask> {
        self.session_tasks.iter_mut().find(|task| task.id == id)
    }

    pub fn update_session_task_status(&mut self, id: &str, status: SessionTaskStatus) -> bool {
        let Some(task) = self.session_task_mut(id) else {
            return false;
        };
        task.transition_to(status);
        self.touch();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_status_display_uses_lowercase_api_names() {
        assert_eq!(TaskStatus::Ready.to_string(), "ready");
        assert_eq!(TaskStatus::Running.to_string(), "running");
        assert_eq!(
            TaskStatus::AwaitingApproval.to_string(),
            "awaiting_approval"
        );
        assert_eq!(TaskStatus::Cancelling.to_string(), "cancelling");
        assert_eq!(TaskStatus::Completed.to_string(), "completed");
        assert_eq!(TaskStatus::Failed.to_string(), "failed");
        assert_eq!(TaskStatus::Cancelled.to_string(), "cancelled");
        assert_eq!(TaskStatus::MaxTurnsReached.to_string(), "max_turns_reached");
    }

    #[test]
    fn cache_usage_stats_accumulate() {
        let mut stats = CacheUsageStats::default();
        stats.total_cache_creation_tokens += 100;
        stats.total_cache_read_tokens += 400;
        stats.total_cache_creation_tokens += 50;
        stats.total_cache_read_tokens += 600;
        assert_eq!(stats.total_cache_creation_tokens, 150);
        assert_eq!(stats.total_cache_read_tokens, 1000);
    }

    #[test]
    fn max_turns_reached_is_distinct_from_completed() {
        assert_ne!(TaskStatus::MaxTurnsReached, TaskStatus::Completed);
        assert_ne!(TaskStatus::MaxTurnsReached, TaskStatus::Cancelled);
        assert_ne!(TaskStatus::MaxTurnsReached, TaskStatus::Failed);
    }

    #[test]
    fn add_and_update_session_task() {
        let mut state = TaskState::new("task-parent".to_string());
        let session_task = SessionTask::new(
            "task-parent",
            "docs-reviewer",
            "review docs",
            Some(PathBuf::from(
                ".vex/state/worktrees/task-parent-docs-reviewer",
            )),
        );
        let session_task_id = session_task.id.clone();

        state.add_session_task(session_task);
        assert_eq!(state.session_tasks.len(), 1);

        assert!(state.update_session_task_status(&session_task_id, SessionTaskStatus::Running));
        assert_eq!(
            state
                .session_task(&session_task_id)
                .map(|task| &task.lifecycle_state),
            Some(&SessionTaskStatus::Running)
        );
    }
}
