//! Session-task join index (ADR-051 Phase 5).
//!
//! One typed JSON document per parent task, persisted at
//! `{task_id}.join.json`. Production join is a single-process
//! orchestrator (`poll_fan_out_join` → `apply_join_outcome`), so a
//! CRDT is not required. Message-id `supersedes` is the replace rule:
//! `live_entries` drops any id that appears in another entry's
//! `supersedes` list. ADR-046 JSONL `PeerMessage` remains the
//! inter-agent log.

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

pub const JOIN_INDEX_SCHEMA_VERSION: u32 = 1;
const JOIN_INDEX_FILE_SUFFIX: &str = "join.json";

/// One live (not superseded) agent handoff after join.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct LiveJoinEntry {
    pub id: String,
    pub agent_id: String,
    pub body: String,
    #[serde(default)]
    pub supersedes: Vec<String>,
}

/// Typed join document persisted at `{task_id}.join.json`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct JoinIndex {
    pub schema_version: u32,
    pub entries: BTreeMap<String, LiveJoinEntry>,
}

pub fn join_index_path(dir: &Path, task_id: &str) -> PathBuf {
    dir.join(format!("{task_id}.{JOIN_INDEX_FILE_SUFFIX}"))
}

pub fn is_join_index_filename(name: &str) -> bool {
    name.ends_with(".join.json")
}

impl JoinIndex {
    pub fn new() -> Self {
        Self {
            schema_version: JOIN_INDEX_SCHEMA_VERSION,
            entries: BTreeMap::new(),
        }
    }

    pub fn post(&mut self, id: &str, agent_id: &str, body: &str, supersedes: &[String]) {
        let entry = self
            .entries
            .entry(id.to_string())
            .or_insert_with(|| LiveJoinEntry {
                id: id.to_string(),
                agent_id: agent_id.to_string(),
                body: body.to_string(),
                supersedes: Vec::new(),
            });
        entry.agent_id = agent_id.to_string();
        entry.body = body.to_string();
        let mut existing: HashSet<String> = entry.supersedes.iter().cloned().collect();
        for superseded_id in supersedes {
            if existing.insert(superseded_id.clone()) {
                entry.supersedes.push(superseded_id.clone());
            }
        }
    }

    /// Entries whose ids do not appear in any `supersedes` list.
    pub fn live_entries(&self) -> Vec<LiveJoinEntry> {
        let mut superseded = HashSet::new();
        for entry in self.entries.values() {
            for superseded_id in &entry.supersedes {
                superseded.insert(superseded_id.clone());
            }
        }
        self.entries
            .values()
            .filter(|entry| !superseded.contains(&entry.id))
            .cloned()
            .collect()
    }

    pub fn save(&self, dir: &Path, task_id: &str) -> Result<()> {
        let path = join_index_path(dir, task_id);
        crate::tools::operator::policy::assert_durable_access(&path)?;
        crate::util::write_json_safe(&path, self, "join index")?;
        Ok(())
    }

    pub fn load(dir: &Path, task_id: &str) -> Result<Self> {
        let path = join_index_path(dir, task_id);
        crate::tools::operator::policy::assert_durable_access(&path)?;
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read join index: {}", path.display()))?;
        let index: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to deserialize join index: {}", path.display()))?;
        Ok(index)
    }

    pub fn try_load(dir: &Path, task_id: &str) -> Result<Option<Self>> {
        if !join_index_path(dir, task_id).is_file() {
            return Ok(None);
        }
        Self::load(dir, task_id).map(Some)
    }

    pub fn load_or_new(dir: &Path, task_id: &str) -> Result<Self> {
        match Self::try_load(dir, task_id)? {
            Some(index) => Ok(index),
            None => Ok(Self::new()),
        }
    }
}

impl Default for JoinIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
pub fn join_index_json_schema_pretty() -> String {
    let schema = schemars::schema_for!(JoinIndex);
    let mut json = serde_json::to_string_pretty(&schema).expect("schema serializes");
    if !json.ends_with('\n') {
        json.push('\n');
    }
    json
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn live_entries_drop_superseded_ids() {
        let mut index = JoinIndex::new();
        index.post("msg-a", "alpha", "first session-task summary", &[]);
        index.post(
            "msg-b",
            "beta",
            "corrected session-task summary",
            &["msg-a".to_string()],
        );
        let live = index.live_entries();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].id, "msg-b");
        assert_eq!(live[0].body, "corrected session-task summary");
        assert!(live.iter().all(|entry| entry.id != "msg-a"));
    }

    #[test]
    fn snapshot_round_trips_through_persist() {
        let dir = TempDir::new().unwrap();
        let mut index = JoinIndex::new();
        index.post("msg-1", "alpha", "persist this body", &[]);
        index.save(dir.path(), "task-join-1").unwrap();
        let loaded = JoinIndex::try_load(dir.path(), "task-join-1")
            .unwrap()
            .expect("sidecar present");
        assert_eq!(loaded.live_entries()[0].body, "persist this body");
    }

    #[test]
    fn try_load_returns_none_when_sidecar_is_absent() {
        let dir = TempDir::new().unwrap();
        assert!(
            JoinIndex::try_load(dir.path(), "missing")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn try_load_errors_when_sidecar_fails_to_deserialize() {
        let dir = TempDir::new().unwrap();
        let path = join_index_path(dir.path(), "corrupt-join");
        std::fs::write(&path, "{not-valid-json").unwrap();
        let err = JoinIndex::try_load(dir.path(), "corrupt-join").unwrap_err();
        assert!(err.to_string().contains("deserialize"));
    }

    #[test]
    fn repost_unions_supersedes_and_keeps_latest_body() {
        let mut index = JoinIndex::new();
        index.post("msg-b", "beta", "first body", &["msg-a".to_string()]);
        index.post(
            "msg-b",
            "beta",
            "corrected body",
            &["msg-a".to_string(), "msg-c".to_string()],
        );
        let live = index.live_entries();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].body, "corrected body");
        assert_eq!(live[0].supersedes.len(), 2);
    }

    #[test]
    fn join_index_schema_matches_checked_in_file() {
        let generated = join_index_json_schema_pretty();
        let schema_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/join_index.schema.json");
        if std::env::var_os("UPDATE_JOIN_INDEX_SCHEMA").is_some() {
            std::fs::write(&schema_path, &generated).expect("write generated schema");
        }
        let expected = include_str!("../../../schemas/join_index.schema.json");
        assert_eq!(expected, generated);
        assert!(generated.contains("https://json-schema.org/draft/2020-12/schema"));
    }

    #[test]
    fn join_index_path_uses_sidecar_suffix() {
        let path = join_index_path(Path::new(".vex/state"), "task-9");
        assert_eq!(path, PathBuf::from(".vex/state/task-9.join.json"));
        assert!(is_join_index_filename("task-9.join.json"));
        assert!(!is_join_index_filename("task-9.json"));
    }
}
