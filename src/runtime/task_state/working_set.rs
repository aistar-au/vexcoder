//! Durable working-set record (ADR-051).
//!
//! The condenser is the sole writer. Persist path:
//! `.vex/state/{task_id}.working-set.json` via [`crate::util::write_json_safe`].
//! Schema is derived with `schemars::schema_for!` (`docs.rs/schemars`).
//! `as_prompt_block` is the serialized form copied onto
//! `ApiClient::set_supplementary_system_prompt` for the next request.

use anyhow::{Context, Result, anyhow};
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

    /// `Ok(None)` when the sidecar is absent. `Err` when a file exists but
    /// durable-access, read, or `serde_json` deserialize fails.
    pub fn try_load(dir: &Path, task_id: &str) -> Result<Option<Self>> {
        if !working_set_path(dir, task_id).is_file() {
            return Ok(None);
        }
        Self::load(dir, task_id).map(Some)
    }

    pub fn load_from_search_dirs_from(working_dir: &Path, task_id: &str) -> Result<Self> {
        match Self::try_load_from_search_dirs_from(working_dir, task_id)? {
            Some(record) => Ok(record),
            None => Err(anyhow!(
                "working-set record for '{task_id}' not found in state search dirs"
            )),
        }
    }

    /// Walks `TaskState::state_search_dirs_from`. Missing sidecar is `Ok(None)`;
    /// a present file that fails to load is `Err`.
    pub fn try_load_from_search_dirs_from(
        working_dir: &Path,
        task_id: &str,
    ) -> Result<Option<Self>> {
        for dir in super::TaskState::state_search_dirs_from(working_dir) {
            if working_set_path(&dir, task_id).is_file() {
                return Self::load(&dir, task_id).map(Some);
            }
        }
        Ok(None)
    }

    /// `objective` is the durable once-set anchor. Later condenser writes keep
    /// the first non-empty value. Episodic fields (`verified_results`,
    /// `changed_paths`) still come from the current pulse window.
    pub fn retain_durable_objective(&mut self, prior: &WorkingSetRecord) {
        let prior_objective = prior.objective.trim();
        if !prior_objective.is_empty() {
            self.objective = prior.objective.clone();
        }
    }

    /// Serialized record copied onto the next request's system prompt.
    /// Field labels match the `serde` `snake_case` names on this type.
    pub fn as_prompt_block(&self) -> String {
        let mut out = String::from("[working-set record: start]\n");
        push_labeled_line(&mut out, "objective", &self.objective);
        push_string_list(&mut out, "constraints", &self.constraints);
        if !self.decisions.is_empty() {
            out.push_str("decisions:\n");
            for decision in &self.decisions {
                out.push_str("- ");
                out.push_str(decision.rationale.trim());
                if !decision.source_reference.trim().is_empty() {
                    out.push_str(" (");
                    out.push_str(decision.source_reference.trim());
                    out.push(')');
                }
                out.push('\n');
            }
        }
        if !self.changed_paths.is_empty() {
            out.push_str("changed_paths:\n");
            for change in &self.changed_paths {
                out.push_str("- ");
                out.push_str(&change.path.display().to_string());
                if !change.git_identity.trim().is_empty() {
                    out.push_str(" (");
                    out.push_str(change.git_identity.trim());
                    out.push(')');
                }
                out.push('\n');
            }
        }
        push_string_list(&mut out, "verified_results", &self.verified_results);
        push_string_list(&mut out, "unresolved_questions", &self.unresolved_questions);
        push_labeled_line(&mut out, "active_plan", &self.active_plan);
        push_labeled_line(&mut out, "next_action", &self.next_action);
        out.push_str("[working-set record: end]");
        out
    }
}

fn push_labeled_line(out: &mut String, label: &str, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    out.push_str(label);
    out.push_str(": ");
    out.push_str(value);
    out.push('\n');
}

fn push_string_list(out: &mut String, label: &str, values: &[String]) {
    let values: Vec<&str> = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect();
    if values.is_empty() {
        return;
    }
    out.push_str(label);
    out.push_str(":\n");
    for value in values {
        out.push_str("- ");
        out.push_str(value);
        out.push('\n');
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
                "restore the next request from the record on /resume".to_string(),
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
        assert!(
            generated.contains("https://json-schema.org/draft/2020-12/schema"),
            "schemars 1.2.2 schema_for! emits JSON Schema 2020-12 (docs.rs/schemars/1.2.2)"
        );
    }

    #[test]
    fn working_set_path_uses_sidecar_suffix() {
        let path = working_set_path(Path::new(".vex/state"), "task-9");
        assert_eq!(path, PathBuf::from(".vex/state/task-9.working-set.json"));
        assert!(is_working_set_filename("task-9.working-set.json"));
        assert!(!is_working_set_filename("task-9.json"));
    }

    #[test]
    fn as_prompt_block_uses_serde_field_names() {
        let block = sample_record().as_prompt_block();
        assert!(block.starts_with("[working-set record: start]\n"));
        assert!(block.contains("objective: persist the working-set record"));
        assert!(block.contains("constraints:\n- local inspectable file"));
        assert!(block.contains(
            "decisions:\n- derive schema from the Rust type (src/runtime/task_state/working_set.rs:1)"
        ));
        assert!(
            block.contains("changed_paths:\n- src/runtime/token_count.rs (token-count-batch1)")
        );
        assert!(block.contains("next_action: load WorkingSetRecord on /resume"));
        assert!(block.ends_with("[working-set record: end]"));
    }

    #[test]
    fn try_load_returns_none_when_sidecar_is_absent() {
        let dir = TempDir::new().unwrap();
        let loaded = WorkingSetRecord::try_load(dir.path(), "missing-task").expect("absent is ok");
        assert!(loaded.is_none());
    }

    #[test]
    fn try_load_errors_when_sidecar_fails_to_deserialize() {
        let dir = TempDir::new().unwrap();
        let path = working_set_path(dir.path(), "corrupt-task");
        std::fs::write(&path, "{not-valid-json").unwrap();
        let err = WorkingSetRecord::try_load(dir.path(), "corrupt-task")
            .expect_err("corrupt sidecar is Err, not Ok(None)");
        let message = err.to_string();
        assert!(
            message.contains("deserialize"),
            "error must name deserialize failure; got {message}"
        );
    }

    #[test]
    fn retain_durable_objective_keeps_first_non_empty_value() {
        let mut later = WorkingSetRecord::new("post-compact unrelated input");
        let prior = WorkingSetRecord::new("original durable objective");
        later.retain_durable_objective(&prior);
        assert_eq!(later.objective, "original durable objective");

        let mut first = WorkingSetRecord::new("first window objective");
        let empty_prior = WorkingSetRecord::new("");
        first.retain_durable_objective(&empty_prior);
        assert_eq!(first.objective, "first window objective");
    }
}
