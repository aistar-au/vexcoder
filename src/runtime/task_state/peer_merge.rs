//! Peer-join merge document (ADR-051 Phase 5).
//!
//! One `LoroDoc` per parent task, persisted as `ExportMode::Snapshot` at
//! `{task_id}.channel.crdt`. Join posts child summaries through
//! `LoroMap::ensure_mergeable_map` / `ensure_mergeable_list`
//! (`docs.rs/loro/1.16.0`). Those APIs give a deterministic child id for
//! `(parent map, key, container type)`, so a re-post of the same message
//! id merges instead of overwriting. `LoroMap::insert_container` is not
//! used: crate docs warn that concurrent same-key container inserts can
//! overwrite rather than merge. `get_or_create_container` is deprecated
//! for the same reason and is not used.
//!
//! Production join is a single-process orchestrator
//! (`poll_fan_out_join` → `apply_join_outcome`). Incremental
//! `ExportMode::updates` / `import` / `oplog_vv` are LoroDoc APIs for
//! independent writers exchanging missing ops. This crate does not wrap
//! them: no second process holds a peer document, and ADR-046 JSONL
//! `PeerMessage` remains the inter-agent log.

use anyhow::{Context, Result, anyhow};
use loro::{ExportMode, LoroDoc};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const MESSAGES_MAP: &str = "messages";
const FIELD_BODY: &str = "body";
const FIELD_AGENT_ID: &str = "agent_id";
const FIELD_SUPERSEDES: &str = "supersedes";
const CRDT_FILE_SUFFIX: &str = "channel.crdt";

/// One live (not superseded) message after CRDT merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePeerEntry {
    pub id: String,
    pub agent_id: String,
    pub body: String,
    pub supersedes: Vec<String>,
}

/// `LoroDoc` wrapper persisted at `{task_id}.channel.crdt`.
pub struct PeerMergeDoc {
    doc: LoroDoc,
}

pub fn peer_merge_path(dir: &Path, task_id: &str) -> PathBuf {
    dir.join(format!("{task_id}.{CRDT_FILE_SUFFIX}"))
}

/// Stable non-zero `PeerID` (`u64`) derived from the task id.
pub fn peer_id_from_task(task_id: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in task_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash.max(1)
}

impl PeerMergeDoc {
    pub fn new(peer_id: u64) -> Result<Self> {
        let doc = LoroDoc::new();
        doc.set_peer_id(peer_id)
            .map_err(|error| anyhow!("LoroDoc::set_peer_id failed: {error}"))?;
        Ok(Self { doc })
    }

    pub fn post(&self, id: &str, agent_id: &str, body: &str, supersedes: &[String]) -> Result<()> {
        let messages = self.doc.get_map(MESSAGES_MAP);
        let entry = messages
            .ensure_mergeable_map(id)
            .map_err(|error| anyhow!("LoroMap::ensure_mergeable_map({id}) failed: {error}"))?;
        entry
            .insert(FIELD_BODY, body)
            .map_err(|error| anyhow!("LoroMap::insert(body) failed: {error}"))?;
        entry
            .insert(FIELD_AGENT_ID, agent_id)
            .map_err(|error| anyhow!("LoroMap::insert(agent_id) failed: {error}"))?;
        let list = entry
            .ensure_mergeable_list(FIELD_SUPERSEDES)
            .map_err(|error| {
                anyhow!("LoroMap::ensure_mergeable_list(supersedes) failed: {error}")
            })?;
        let mut existing = HashSet::new();
        for index in 0..list.len() {
            if let Some(item) = list_item_as_string(&list, index) {
                existing.insert(item);
            }
        }
        for superseded_id in supersedes {
            if existing.contains(superseded_id) {
                continue;
            }
            list.push(superseded_id.as_str())
                .map_err(|error| anyhow!("LoroList::push(supersedes) failed: {error}"))?;
            existing.insert(superseded_id.clone());
        }
        self.doc.commit();
        Ok(())
    }

    /// Entries whose ids do not appear in any `supersedes` list.
    pub fn live_entries(&self) -> Vec<LivePeerEntry> {
        let Some(messages) = root_messages_object(&self.doc) else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        let mut superseded = HashSet::new();
        for (id, value) in messages {
            let Some(entry) = parse_entry(&id, value) else {
                continue;
            };
            for superseded_id in &entry.supersedes {
                superseded.insert(superseded_id.clone());
            }
            entries.push(entry);
        }
        entries.retain(|entry| !superseded.contains(&entry.id));
        entries
    }

    pub fn export_snapshot(&self) -> Result<Vec<u8>> {
        self.doc
            .export(ExportMode::Snapshot)
            .map_err(|error| anyhow!("LoroDoc::export(Snapshot) failed: {error}"))
    }

    pub fn save(&self, dir: &Path, task_id: &str) -> Result<()> {
        let path = peer_merge_path(dir, task_id);
        crate::tools::operator::policy::assert_durable_access(&path)?;
        let bytes = self.export_snapshot()?;
        crate::util::write_bytes_safe(&path, &bytes, "peer merge document")
    }

    pub fn try_load(dir: &Path, task_id: &str, peer_id: u64) -> Result<Option<Self>> {
        let path = peer_merge_path(dir, task_id);
        if !path.is_file() {
            return Ok(None);
        }
        crate::tools::operator::policy::assert_durable_access(&path)?;
        let bytes = std::fs::read(&path)
            .with_context(|| format!("Failed to read peer merge document: {}", path.display()))?;
        let doc = LoroDoc::from_snapshot(&bytes)
            .map_err(|error| anyhow!("LoroDoc::from_snapshot failed: {error}"))?;
        doc.set_peer_id(peer_id)
            .map_err(|error| anyhow!("LoroDoc::set_peer_id after snapshot failed: {error}"))?;
        Ok(Some(Self { doc }))
    }

    pub fn load_or_new(dir: &Path, task_id: &str) -> Result<Self> {
        let peer_id = peer_id_from_task(task_id);
        match Self::try_load(dir, task_id, peer_id)? {
            Some(doc) => Ok(doc),
            None => Self::new(peer_id),
        }
    }
}

fn list_item_as_string(list: &loro::LoroList, index: usize) -> Option<String> {
    let value = list.get(index)?;
    let json = serde_json::to_value(value.get_deep_value()).ok()?;
    json.as_str().map(str::to_string)
}

fn root_messages_object(doc: &LoroDoc) -> Option<serde_json::Map<String, serde_json::Value>> {
    let root = doc.get_deep_value();
    let json = loro_value_to_json(&root);
    json.get(MESSAGES_MAP)?.as_object().cloned()
}

fn loro_value_to_json(value: &loro::LoroValue) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

fn parse_entry(id: &str, value: serde_json::Value) -> Option<LivePeerEntry> {
    let object = value.as_object()?;
    let body = object
        .get(FIELD_BODY)
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let agent_id = object
        .get(FIELD_AGENT_ID)
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let supersedes = object
        .get(FIELD_SUPERSEDES)
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some(LivePeerEntry {
        id: id.to_string(),
        agent_id,
        body,
        supersedes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn live_entries_drop_superseded_ids() {
        let doc = PeerMergeDoc::new(1).expect("new doc");
        doc.post("msg-a", "alpha", "first child summary", &[])
            .unwrap();
        doc.post(
            "msg-b",
            "beta",
            "corrected child summary",
            &["msg-a".to_string()],
        )
        .unwrap();
        let live = doc.live_entries();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].id, "msg-b");
        assert_eq!(live[0].body, "corrected child summary");
        assert!(
            live.iter().all(|entry| entry.id != "msg-a"),
            "superseded id must not remain live"
        );
    }

    #[test]
    fn snapshot_round_trips_through_persist() {
        let dir = TempDir::new().unwrap();
        let doc = PeerMergeDoc::new(peer_id_from_task("task-crdt-1")).unwrap();
        doc.post("msg-1", "alpha", "persist this body", &[])
            .unwrap();
        doc.save(dir.path(), "task-crdt-1").unwrap();
        let loaded =
            PeerMergeDoc::try_load(dir.path(), "task-crdt-1", peer_id_from_task("task-crdt-1"))
                .unwrap()
                .expect("sidecar present");
        let live = loaded.live_entries();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].body, "persist this body");
    }

    #[test]
    fn try_load_returns_none_when_sidecar_is_absent() {
        let dir = TempDir::new().unwrap();
        let loaded = PeerMergeDoc::try_load(dir.path(), "missing", 1).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn ensure_mergeable_repost_unions_supersedes_and_keeps_latest_body() {
        let doc = PeerMergeDoc::new(1).unwrap();
        doc.post("msg-b", "beta", "first body", &["msg-a".to_string()])
            .unwrap();
        doc.post(
            "msg-b",
            "beta",
            "corrected body",
            &["msg-a".to_string(), "msg-c".to_string()],
        )
        .unwrap();
        let live = doc.live_entries();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].id, "msg-b");
        assert_eq!(live[0].body, "corrected body");
        assert!(live[0].supersedes.contains(&"msg-a".to_string()));
        assert!(live[0].supersedes.contains(&"msg-c".to_string()));
        assert_eq!(live[0].supersedes.len(), 2);
    }
}
