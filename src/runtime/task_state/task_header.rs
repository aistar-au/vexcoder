use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

use crate::runtime::session_task::SessionTaskStatus;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TaskStateHeader {
    pub id: String,

    #[serde(rename = "updated_at", default)]
    pub modified_millis: u64,

    #[serde(default)]
    pub session_tasks: Option<Vec<SessionTaskSummary>>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SessionTaskSummary {
    pub agent_id: String,
    #[serde(rename = "lifecycle_state")]
    pub status: SessionTaskStatus,
}

impl TaskStateHeader {
    pub fn from_path(path: &Path) -> Result<Self> {
        crate::tools::operator::policy::assert_durable_access(path)?;
        let file = std::fs::File::open(path)
            .with_context(|| format!("failed to open task state file: {}", path.display()))?;
        let reader = std::io::BufReader::new(file);
        serde_json::from_reader(reader)
            .with_context(|| format!("failed to parse task-state header: {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_state_file(dir: &TempDir, name: &str, json: &str) -> std::path::PathBuf {
        let path = dir.path().join(format!("{name}.json"));
        std::fs::write(&path, json).unwrap();
        path
    }

    #[test]
    fn parses_id_and_updated_at() {
        let dir = TempDir::new().unwrap();
        let path = write_state_file(
            &dir,
            "t1",
            r#"{"id":"t1","status":"Ready","updated_at":1000,"active_grants":{},"changed_files":[],"command_history":[],"conversation_snapshot":{"message_count":0,"summary":""},"interrupted_sessions":[]}"#,
        );
        let header = TaskStateHeader::from_path(&path).unwrap();
        assert_eq!(header.id, "t1");
        assert_eq!(header.modified_millis, 1000);
        assert!(header.session_tasks.is_none());
    }

    #[test]
    fn defaults_updated_at_to_zero_for_older_json() {
        let dir = TempDir::new().unwrap();

        let path = write_state_file(
            &dir,
            "older-format",
            r#"{"id":"older-format","status":"Completed","active_grants":{},"changed_files":[],"command_history":[],"conversation_snapshot":{"message_count":0,"summary":""},"interrupted_sessions":[]}"#,
        );
        let header = TaskStateHeader::from_path(&path).unwrap();
        assert_eq!(header.id, "older-format");
        assert_eq!(header.modified_millis, 0);
    }

    #[test]
    fn parses_session_tasks_liveness() {
        let dir = TempDir::new().unwrap();
        let path = write_state_file(
            &dir,
            "multi",
            r#"{"id":"multi","status":"Running","updated_at":2000,"active_grants":{},"changed_files":[],"command_history":[],"conversation_snapshot":{"message_count":0,"summary":""},"interrupted_sessions":[],"session_tasks":[{"id":"multi-reviewer-abc","parent_task_id":"multi","agent_id":"reviewer","prompt":"review","lifecycle_state":"Running","updated_at":2000}]}"#,
        );
        let header = TaskStateHeader::from_path(&path).unwrap();
        let tasks = header.session_tasks.unwrap();
        assert_eq!(tasks.len(), 1);
        assert!(tasks[0].status.is_live());
        assert_eq!(tasks[0].agent_id, "reviewer");
    }

    #[test]
    fn ignores_large_fields_without_allocating_them() {
        let dir = TempDir::new().unwrap();
        let mut big = String::from(
            r#"{"id":"big","status":"Completed","updated_at":9,"active_grants":{},"changed_files":[],"command_history":[],"conversation_snapshot":{"message_count":0,"summary":""},"interrupted_sessions":[],"pulses":["#,
        );
        for i in 0..100 {
            if i > 0 {
                big.push(',');
            }
            big.push_str(r#"{"input":"x","response":"y","changed_files":[],"command_history":[],"tool_invocations":[],"tokens":{"input":0,"output":0}}"#);
        }
        big.push_str("]}");
        let path = write_state_file(&dir, "big", &big);
        let header = TaskStateHeader::from_path(&path).unwrap();
        assert_eq!(header.id, "big");
    }
}
