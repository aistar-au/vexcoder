//! Peer-join merge document (ADR-051 Phase 5).
//!
//! `LoroDoc` is the CRDT surface (`docs.rs/loro`): `get_map`,
//! `insert_container`, `export(ExportMode::…)`, `import`, `oplog_vv`.
//! ADR-046 JSONL `PeerMessage` routes stay in `peer_channel.rs`. This
//! type is the join merge rule: message-id supersession instead of
//! concatenating every child `handoff_summary`.

use anyhow::{Context, Result, anyhow};
use loro::{ExportMode, LoroDoc, LoroList, LoroMap, VersionVector};
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
            .insert_container(id, LoroMap::new())
            .map_err(|error| anyhow!("LoroMap::insert_container({id}) failed: {error}"))?;
        entry
            .insert(FIELD_BODY, body)
            .map_err(|error| anyhow!("LoroMap::insert(body) failed: {error}"))?;
        entry
            .insert(FIELD_AGENT_ID, agent_id)
            .map_err(|error| anyhow!("LoroMap::insert(agent_id) failed: {error}"))?;
        let list = entry
            .insert_container(FIELD_SUPERSEDES, LoroList::new())
            .map_err(|error| anyhow!("LoroMap::insert_container(supersedes) failed: {error}"))?;
        for (index, superseded_id) in supersedes.iter().enumerate() {
            list.insert(index, superseded_id.as_str())
                .map_err(|error| {
                    anyhow!("LoroList::insert(supersedes[{index}]) failed: {error}")
                })?;
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

    pub fn export_updates_since(&self, since: &VersionVector) -> Result<Vec<u8>> {
        self.doc
            .export(ExportMode::updates(since))
            .map_err(|error| anyhow!("LoroDoc::export(Updates) failed: {error}"))
    }

    pub fn import_updates(&self, bytes: &[u8]) -> Result<()> {
        self.doc
            .import(bytes)
            .map_err(|error| anyhow!("LoroDoc::import failed: {error}"))?;
        Ok(())
    }

    pub fn oplog_vv(&self) -> VersionVector {
        self.doc.oplog_vv()
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
    fn export_updates_import_on_second_peer() {
        let alpha = PeerMergeDoc::new(11).unwrap();
        let beta = PeerMergeDoc::new(22).unwrap();
        alpha.post("msg-a", "alpha", "from alpha", &[]).unwrap();
        let since = beta.oplog_vv();
        let updates = alpha.export_updates_since(&since).unwrap();
        beta.import_updates(&updates).unwrap();
        let live = beta.live_entries();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].body, "from alpha");
    }

    #[test]
    fn try_load_returns_none_when_sidecar_is_absent() {
        let dir = TempDir::new().unwrap();
        let loaded = PeerMergeDoc::try_load(dir.path(), "missing", 1).unwrap();
        assert!(loaded.is_none());
    }
}
