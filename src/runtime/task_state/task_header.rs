use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

use crate::runtime::session_task::SessionTaskStatus;

/// Minimal header deserialised from a task-state JSON file.
///
/// Used for cold-start discovery, UI recent-task lists, and session-check
/// scans. Only the fields required for those surfaces are present; all
/// other TaskState fields are ignored by serde and never allocated.
///
/// NOTE ON PERFORMANCE: serde_json processes the full JSON stream even for
/// small structs — there is no built-in early-exit for derived Deserialize
/// impls. The performance gain comes from not constructing the large
/// sub-graphs that a full TaskState::load() would allocate:
/// `Vec<TurnEvidenceState>`, `BTreeMap<Capability, ApprovalScope>`,
/// `Vec<InterruptedCommand>`, `Vec<ContextCompactionRecord>`,
/// `Vec<SessionNote>`. None of those types are referenced here, so serde
/// discards their JSON representations without allocating.
///
/// Compatible with pre-ADR-045 TaskState JSON via #[serde(default)].
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TaskStateHeader {
    pub id: String,
    /// Maps to TaskState.updated_at — milliseconds since the Unix epoch.
    /// u64 matches the TaskState field type; note TaskStateFile.modified_millis
    /// is u128 (filesystem mtime) and may differ for externally edited files.
    #[serde(rename = "updated_at", default)]
    pub modified_millis: u64,
    /// Present only when the task has session sub-tasks (multi-agent flows).
    /// None for pre-ADR-034 task files that predate the session_tasks field.
    #[serde(default)]
    pub session_tasks: Option<Vec<SessionTaskSummary>>,
}

/// Minimal projection of a SessionTask sufficient for liveness checks.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SessionTaskSummary {
    pub agent_id: String,
    #[serde(rename = "lifecycle_state")]
    pub status: SessionTaskStatus,
}

impl TaskStateHeader {
    /// Open `path` and deserialise only the header projection.
    ///
    /// Calls `assert_durable_access` before opening (ADR-038 Batch G
    /// disk-policy requirement).
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
    fn defaults_updated_at_to_zero_for_legacy_json() {
        let dir = TempDir::new().unwrap();
        // Pre-ADR-030 fixture: no updated_at field
        let path = write_state_file(
            &dir,
            "legacy",
            r#"{"id":"legacy","status":"Completed","active_grants":{},"changed_files":[],"command_history":[],"conversation_snapshot":{"message_count":0,"summary":""},"interrupted_sessions":[]}"#,
        );
        let header = TaskStateHeader::from_path(&path).unwrap();
        assert_eq!(header.id, "legacy");
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
        // Regression guard: a JSON with large turns/command_history arrays must
        // parse successfully into the small header without error.
        let dir = TempDir::new().unwrap();
        let mut big = String::from(
            r#"{"id":"big","status":"Completed","updated_at":9,"active_grants":{},"changed_files":[],"command_history":[],"conversation_snapshot":{"message_count":0,"summary":""},"interrupted_sessions":[],"turns":["#,
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
