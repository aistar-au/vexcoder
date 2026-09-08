//! Versioned state envelope for internal consumers (ADR-051).
//!
//! Callers load continuity through this type instead of opening
//! `{id}.json`, `{id}.working-set.json`, or `{id}.join.json` directly.

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::join_index::{JoinIndex, join_index_path};
use super::working_set::{WorkingSetRecord, working_set_path};

pub const STATE_ENVELOPE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct StateRefs {
    pub task_id: String,
    pub task_state: String,
    pub working_set: String,
    pub join_index: String,
}

impl StateRefs {
    pub fn for_task(task_id: &str) -> Self {
        Self {
            task_id: task_id.to_string(),
            task_state: format!("{task_id}.json"),
            working_set: format!("{task_id}.working-set.json"),
            join_index: format!("{task_id}.join.json"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct StateEnvelope {
    pub schema_version: u32,
    pub task_id: String,
    pub refs: StateRefs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_set: Option<WorkingSetRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<JoinIndex>,
}

impl StateEnvelope {
    pub fn load_for_task(dir: &Path, task_id: &str) -> Result<Self> {
        let task_path = dir.join(format!("{task_id}.json"));
        if !task_path.is_file() {
            anyhow::bail!("task state for '{task_id}' not found");
        }
        crate::tools::operator::policy::assert_durable_access(&task_path)?;
        let working_set = WorkingSetRecord::try_load(dir, task_id)
            .with_context(|| format!("working-set load failed for {task_id}"))?;
        let join = JoinIndex::try_load(dir, task_id)
            .with_context(|| format!("join index load failed for {task_id}"))?;
        Ok(Self {
            schema_version: STATE_ENVELOPE_SCHEMA_VERSION,
            task_id: task_id.to_string(),
            refs: StateRefs::for_task(task_id),
            working_set,
            join,
        })
    }

    pub fn working_set_path(dir: &Path, task_id: &str) -> PathBuf {
        working_set_path(dir, task_id)
    }

    pub fn join_index_path(dir: &Path, task_id: &str) -> PathBuf {
        join_index_path(dir, task_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::task_state::TaskState;
    use tempfile::TempDir;

    #[test]
    fn envelope_loads_typed_sidecars_without_opening_paths_in_caller() {
        let dir = TempDir::new().unwrap();
        TaskState::new("task-env-1".to_string())
            .save(dir.path())
            .unwrap();
        WorkingSetRecord::new("keep the objective")
            .save(dir.path(), "task-env-1")
            .unwrap();
        let mut join = JoinIndex::new();
        join.post("msg-1", "alpha", "session-task summary", &[]);
        join.save(dir.path(), "task-env-1").unwrap();
        let envelope = StateEnvelope::load_for_task(dir.path(), "task-env-1").unwrap();
        assert_eq!(envelope.refs.join_index, "task-env-1.join.json");
        assert_eq!(
            envelope.working_set.as_ref().map(|r| r.objective.as_str()),
            Some("keep the objective")
        );
        assert_eq!(
            envelope.join.as_ref().map(|i| i.live_entries().len()),
            Some(1)
        );
    }

    #[test]
    fn envelope_errors_when_task_state_is_missing() {
        let dir = TempDir::new().unwrap();
        let err = StateEnvelope::load_for_task(dir.path(), "missing").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }
}
