use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::runtime::{ApprovalScope, Capability};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskState {
    pub id: TaskId,
    pub status: TaskStatus,
    pub active_grants: HashMap<Capability, ApprovalScope>,
    pub changed_files: Vec<PathBuf>,
    pub command_history: Vec<CommandEvidence>,
    pub conversation_snapshot: ConversationCheckpoint,
    pub interrupted_sessions: Vec<InterruptedCommand>,
}

impl TaskState {
    pub fn new(id: TaskId) -> Self {
        Self {
            id,
            status: TaskStatus::Ready,
            active_grants: HashMap::new(),
            changed_files: Vec::new(),
            command_history: Vec::new(),
            conversation_snapshot: ConversationCheckpoint::default(),
            interrupted_sessions: Vec::new(),
        }
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create state directory: {}", dir.display()))?;

        let temp_path = dir.join(format!("{}.tmp", self.id));
        let final_path = dir.join(format!("{}.json", self.id));

        let json = serde_json::to_vec_pretty(self).context("Failed to serialize task state")?;
        let mut file = std::fs::File::create(&temp_path)
            .with_context(|| format!("Failed to create temp state file: {}", temp_path.display()))?;
        file.write_all(&json)
            .with_context(|| format!("Failed to write temp state file: {}", temp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("Failed to flush temp state file: {}", temp_path.display()))?;
        drop(file);

        std::fs::rename(&temp_path, &final_path)
            .with_context(|| format!("Failed to rename state file to: {}", final_path.display()))?;

        Ok(())
    }

    pub fn load(dir: &Path, id: &str) -> Result<Self> {
        let path = dir.join(format!("{}.json", id));
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read state file: {}", path.display()))?;

        let mut state: TaskState = serde_json::from_str(&content)
            .with_context(|| format!("Failed to deserialize state file: {}", path.display()))?;

        for evidence in &mut state.command_history {
            if evidence.exit_code.is_none() {
                evidence.interrupted = true;
            }
        }

        Ok(state)
    }

    pub fn state_dir() -> PathBuf {
        std::env::var("VEX_STATE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(".vex/state"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_task_state_survives_atomic_write_and_reload() {
        let dir = TempDir::new().unwrap();
        let state = TaskState {
            id: "task-001".to_string(),
            status: TaskStatus::Completed,
            active_grants: HashMap::from([(Capability::ApplyPatch, ApprovalScope::Once)]),
            changed_files: vec![PathBuf::from("src/main.rs")],
            command_history: vec![CommandEvidence {
                program: "cargo test".into(),
                exit_code: None,
                interrupted: true,
            }],
            conversation_snapshot: ConversationCheckpoint::default(),
            interrupted_sessions: vec![InterruptedCommand {
                program: "cargo build".into(),
                interrupted_at: "2026-03-01T00:00:00Z".into(),
            }],
        };

        state.save(dir.path()).expect("save failed");
        let loaded = TaskState::load(dir.path(), "task-001").expect("load failed");

        assert_eq!(loaded.status, TaskStatus::Completed);
        assert_eq!(loaded.changed_files, state.changed_files);
        assert!(loaded.command_history[0].interrupted);
        assert_eq!(loaded.interrupted_sessions.len(), 1);
    }

    #[test]
    fn test_task_state_marks_interrupted_commands_on_reload() {
        let dir = TempDir::new().unwrap();
        let state = TaskState {
            id: "task-456".to_string(),
            status: TaskStatus::Running,
            active_grants: HashMap::new(),
            changed_files: Vec::new(),
            command_history: vec![CommandEvidence {
                program: "sleep 100".to_string(),
                exit_code: None,
                interrupted: false,
            }],
            conversation_snapshot: ConversationCheckpoint::default(),
            interrupted_sessions: Vec::new(),
        };

        state.save(dir.path()).expect("save failed");
        let loaded = TaskState::load(dir.path(), "task-456").expect("load failed");
        assert_eq!(loaded.command_history.len(), 1);
        assert!(loaded.command_history[0].interrupted);
    }
}
