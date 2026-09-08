//! Durable working-set record (ADR-051 Batch 1).
//!
//! The condenser is the sole writer. Persist path:
//! `.vex/state/{task_id}.working-set.json` via [`crate::util::write_json_safe`].
//! Schema is derived with `schemars::schema_for!` (`docs.rs/schemars`).

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const WORKING_SET_SCHEMA_VERSION: u32 = 1;
const WORKING_SET_FILE_SUFFIX: &str = "working-set.json";

/// Durable unit of continuity, persisted next to the task-state file.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct WorkingSetRecord {
    pub schema_version: u32,
    pub updated_at: DateTime<Utc>,
    pub objective: String,
    pub constraints: Vec<String>,
    pub decisions: Vec<RecordedDecision>,
    pub changed_paths: Vec<PathChange>,
    pub verified_results: Vec<String>,
    pub unresolved_questions: Vec<String>,
    pub active_plan: String,
    pub next_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct RecordedDecision {
    pub rationale: String,
    /// File location or a peer-channel message id.
    pub source_reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PathChange {
    pub path: PathBuf,
    pub git_identity: String,
}

pub fn working_set_path(dir: &Path, task_id: &str) -> PathBuf {
    dir.join(format!("{task_id}.{WORKING_SET_FILE_SUFFIX}"))
}

pub fn is_working_set_filename(name: &str) -> bool {
    name.ends_with(".working-set.json")
}

impl WorkingSetRecord {
    pub fn new(objective: impl Into<String>) -> Self {
        Self {
            schema_version: WORKING_SET_SCHEMA_VERSION,
            updated_at: Utc::now(),
            objective: objective.into(),
            constraints: Vec::new(),
            decisions: Vec::new(),
            changed_paths: Vec::new(),
            verified_results: Vec::new(),
            unresolved_questions: Vec::new(),
            active_plan: String::new(),
            next_action: String::new(),
        }
    }

    pub fn save(&self, dir: &Path, task_id: &str) -> Result<()> {
        let path = working_set_path(dir, task_id);
        crate::tools::operator::policy::assert_durable_access(&path)?;
        crate::util::write_json_safe(&path, self, "working-set record")?;
        Ok(())
    }

    pub fn load(dir: &Path, task_id: &str) -> Result<Self> {
        let path = working_set_path(dir, task_id);
        crate::tools::operator::policy::assert_durable_access(&path)?;
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read working-set record: {}", path.display()))?;
        let record: Self = serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to deserialize working-set record: {}",
                path.display()
            )
        })?;
        Ok(record)
    }
}

/// Pretty JSON Schema generated from the Rust type. Used by the drift test.
#[cfg(test)]
pub fn working_set_json_schema_pretty() -> String {
    let schema = schemars::schema_for!(WorkingSetRecord);
    let mut json = serde_json::to_string_pretty(&schema).expect("schema serializes");
    if !json.ends_with('\n') {
        json.push('\n');
    }
    json
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tempfile::TempDir;

    fn sample_record() -> WorkingSetRecord {
        WorkingSetRecord {
            schema_version: WORKING_SET_SCHEMA_VERSION,
            updated_at: Utc.timestamp_opt(1_757_289_600, 0).unwrap(),
            objective: "persist the working-set record".to_string(),
            constraints: vec!["local inspectable file".to_string()],
            decisions: vec![RecordedDecision {
                rationale: "derive schema from the Rust type".to_string(),
                source_reference: "src/runtime/task_state/working_set.rs:1".to_string(),
            }],
            changed_paths: vec![PathChange {
                path: PathBuf::from("src/runtime/token_count.rs"),
                git_identity: "token-count-batch1".to_string(),
            }],
            verified_results: vec!["round-trip through persist".to_string()],
            unresolved_questions: vec![
                "seed the next request from the record in a later batch".to_string(),
            ],
            active_plan: "schema, persist, token count".to_string(),
            next_action: "load WorkingSetRecord on /resume".to_string(),
        }
    }

    #[test]
    fn working_set_record_round_trips_through_persist() {
        let dir = TempDir::new().unwrap();
        let record = sample_record();
        record.save(dir.path(), "task-ws-001").expect("save failed");
        let loaded = WorkingSetRecord::load(dir.path(), "task-ws-001").expect("load failed");
        assert_eq!(loaded, record);
        assert!(working_set_path(dir.path(), "task-ws-001").is_file());
    }

    #[test]
    fn working_set_schema_matches_checked_in_file() {
        let generated = working_set_json_schema_pretty();
        let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("schemas/working_set.schema.json");
        if std::env::var_os("UPDATE_WORKING_SET_SCHEMA").is_some() {
            std::fs::write(&schema_path, &generated).expect("write generated schema");
        }
        let expected = include_str!("../../../schemas/working_set.schema.json");
        assert_eq!(
            expected, generated,
            "schemas/working_set.schema.json drifted from schema_for!(WorkingSetRecord); rerun with UPDATE_WORKING_SET_SCHEMA=1"
        );
    }

    #[test]
    fn working_set_path_uses_sidecar_suffix() {
        let path = working_set_path(Path::new(".vex/state"), "task-9");
        assert_eq!(path, PathBuf::from(".vex/state/task-9.working-set.json"));
        assert!(is_working_set_filename("task-9.working-set.json"));
        assert!(!is_working_set_filename("task-9.json"));
    }
}
