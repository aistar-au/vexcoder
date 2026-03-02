use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub type TaskId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEvidence {
    pub command: String,
    pub args: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub interrupted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationCheckpoint {
    pub message_count: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterruptedCommand {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub id: TaskId,
    pub status: TaskStatus,
    pub working_dir: PathBuf,
    pub changed_files: Vec<String>,
    pub command_evidence: Vec<CommandEvidence>,
    pub conversation_checkpoint: Option<ConversationCheckpoint>,
    pub interrupted_commands: Vec<InterruptedCommand>,
    pub metadata: HashMap<String, String>,
}

impl TaskState {
    pub fn new(id: TaskId, working_dir: PathBuf) -> Self {
        Self {
            id,
            status: TaskStatus::Running,
            working_dir,
            changed_files: Vec::new(),
            command_evidence: Vec::new(),
            conversation_checkpoint: None,
            interrupted_commands: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        // Create directory if it doesn't exist
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create state directory: {}", dir.display()))?;

        let temp_path = dir.join(format!("{}.tmp", self.id));
        let final_path = dir.join(format!("{}.json", self.id));

        // Serialize to JSON
        let json = serde_json::to_string_pretty(self).context("Failed to serialize task state")?;

        // Write to temp file
        std::fs::write(&temp_path, json)
            .with_context(|| format!("Failed to write temp state file: {}", temp_path.display()))?;

        // Atomic rename
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

        // On reload, mark commands with exit_code: None as interrupted
        for evidence in &mut state.command_evidence {
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
        let task = TaskState::new("task-123".to_string(), PathBuf::from("/workspace"));

        // Save task state
        task.save(dir.path()).expect("save failed");

        // Reload and verify
        let loaded = TaskState::load(dir.path(), "task-123").expect("load failed");
        assert_eq!(loaded.id, "task-123");
        assert_eq!(loaded.status, TaskStatus::Running);
        assert!(matches!(loaded.status, TaskStatus::Running));
    }

    #[test]
    fn test_task_state_marks_interrupted_commands_on_reload() {
        let dir = TempDir::new().unwrap();
        let mut task = TaskState::new("task-456".to_string(), PathBuf::from("/workspace"));

        // Add a command with no exit code (simulating interruption)
        task.command_evidence.push(CommandEvidence {
            command: "sleep".to_string(),
            args: vec!["100".to_string()],
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            interrupted: false,
        });

        // Save task state
        task.save(dir.path()).expect("save failed");

        // Reload and verify interrupted flag is set
        let loaded = TaskState::load(dir.path(), "task-456").expect("load failed");
        assert_eq!(loaded.command_evidence.len(), 1);
        assert!(loaded.command_evidence[0].interrupted);
    }
}
