//! Append-only file-backed message channel between session tasks.
//!
//! Each parent task owns a sidecar JSONL file at
//! `.vex/state/{parent_task_id}.channel.jsonl`. Agents sharing a parent task
//! post observations, corrections, and questions through this channel. The
//! channel is designed for single-digit concurrency and uses the same
//! two-layer locking pattern (`in-process Mutex` + cross-process `flock`)
//! that `task_facade.rs` uses for delegation writes.
//!
//! All public operations go through the application facade (ADR-028).
//! This module provides only types and low-level read/write primitives.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::runtime::session_task::now_millis;

/// Maximum message body size in bytes.
pub const MAX_PEER_MESSAGE_BYTES: usize = 4_096;
/// Maximum messages stored in a single channel file.
pub const MAX_CHANNEL_DEPTH: usize = 256;
/// Maximum messages returned in a single read batch.
pub const MAX_CHANNEL_READ_BATCH: usize = 64;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Semantic classification of a peer message.
///
/// Kinds are informational — the runtime does not act on them directly.
/// The receiving agent decides how to incorporate the message based on
/// its own reasoning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PeerMessageKind {
    /// The sender shares a factual observation relevant to the task.
    Observation,
    /// The sender proposes a correction to work in progress.
    Correction,
    /// The sender asks for input before proceeding.
    Question,
    /// The sender acknowledges a prior message and records its response.
    Acknowledgement,
}

impl std::fmt::Display for PeerMessageKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Observation => f.write_str("observation"),
            Self::Correction => f.write_str("correction"),
            Self::Question => f.write_str("question"),
            Self::Acknowledgement => f.write_str("acknowledgement"),
        }
    }
}

/// A single peer message appended to a parent task's channel file.
///
/// ADR-045 note: when the event-log architecture lands, `PeerMessagePosted`
/// will wrap this type as a `RuntimeEvent` variant. Until then messages are
/// persisted only in the sidecar JSONL file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMessage {
    /// Unique message identifier.
    pub id: String,
    /// Epoch milliseconds — used as the pagination cursor.
    pub sent_at: u64,
    /// Full session task ID of the sender (from `SessionTask.id`).
    pub sender_id: String,
    /// Human-readable agent name (from `SessionTask.agent_id`).
    pub sender_agent_id: String,
    /// `"*"` for broadcast, or a specific `agent_id` for point-to-point.
    pub recipient: String,
    /// Semantic kind — informational only.
    pub kind: PeerMessageKind,
    /// Message body. Must not exceed `MAX_PEER_MESSAGE_BYTES`.
    pub content: String,
    /// Parent task this channel belongs to.
    pub parent_task_id: String,
}

impl PeerMessage {
    pub fn new(
        sender_id: impl Into<String>,
        sender_agent_id: impl Into<String>,
        parent_task_id: impl Into<String>,
        recipient: impl Into<String>,
        kind: PeerMessageKind,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().as_hyphenated().to_string(),
            sent_at: now_millis(),
            sender_id: sender_id.into(),
            sender_agent_id: sender_agent_id.into(),
            recipient: recipient.into(),
            kind,
            content: content.into(),
            parent_task_id: parent_task_id.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// File path helper
// ---------------------------------------------------------------------------

/// Returns the channel file path for the given state directory and parent
/// task ID.
pub fn channel_path(state_dir: &Path, parent_task_id: &str) -> PathBuf {
    state_dir.join(format!("{parent_task_id}.channel.jsonl"))
}

// ---------------------------------------------------------------------------
// Write
// ---------------------------------------------------------------------------

/// Append one message to the channel file.
///
/// Enforces `MAX_CHANNEL_DEPTH` before writing. Callers must acquire the
/// channel write lock before calling this function to prevent concurrent
/// write corruption on macOS and Windows where POSIX O_APPEND atomicity
/// guarantees are weaker than on Linux.
///
/// Does NOT call `assert_durable_access` — callers must validate the path
/// through the facade (ADR-028 boundary).
pub fn append_message(state_dir: &Path, message: &PeerMessage) -> Result<()> {
    let path = channel_path(state_dir, &message.parent_task_id);
    std::fs::create_dir_all(state_dir)
        .with_context(|| format!("failed to create state dir: {}", state_dir.display()))?;

    let existing = count_lines(&path)?;
    if existing >= MAX_CHANNEL_DEPTH {
        anyhow::bail!(
            "channel for task '{}' is full ({} messages); \
             wait for the orchestrator to archive before posting",
            message.parent_task_id,
            existing
        );
    }

    let line = serde_json::to_string(message).context("failed to serialise peer message")?;

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open channel file: {}", path.display()))?;

    writeln!(file, "{line}")
        .with_context(|| format!("failed to append to channel file: {}", path.display()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Read
// ---------------------------------------------------------------------------

/// Read up to `MAX_CHANNEL_READ_BATCH` messages from the channel, optionally
/// filtered by recipient and paginated by `after_ms`.
///
/// `after_ms` is the `sent_at` value of the last message the caller has
/// already seen. Pass `0` to read from the beginning of the channel.
///
/// `recipient_filter` — when `Some(agent_id)`, returns only messages where
/// `recipient == "*"` or `recipient == agent_id`. When `None`, returns all
/// messages (used by the orchestrator for audit/watch surfaces).
pub fn read_messages(
    state_dir: &Path,
    parent_task_id: &str,
    after_ms: u64,
    recipient_filter: Option<&str>,
) -> Result<Vec<PeerMessage>> {
    let path = channel_path(state_dir, parent_task_id);
    if !path.exists() {
        return Ok(Vec::new());
    }

    crate::tools::operator::policy::assert_durable_access(&path)?;

    let file = std::fs::File::open(&path)
        .with_context(|| format!("failed to open channel file: {}", path.display()))?;
    let reader = BufReader::new(file);

    let mut results = Vec::with_capacity(MAX_CHANNEL_READ_BATCH.min(32));

    for line in reader.lines() {
        let line = line
            .with_context(|| format!("failed to read channel file line: {}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }

        let msg: PeerMessage = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!(
                    path = %path.display(),
                    error = %e,
                    "skipping malformed line in channel file"
                );
                continue;
            }
        };

        if msg.sent_at <= after_ms {
            continue;
        }

        if let Some(agent_id) = recipient_filter {
            if msg.recipient != "*" && msg.recipient != agent_id {
                continue;
            }
        }

        results.push(msg);
        if results.len() >= MAX_CHANNEL_READ_BATCH {
            break;
        }
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn count_lines(path: &Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to open channel file for count: {}", path.display()))?;
    let count = BufReader::new(file)
        .lines()
        .filter(|l| l.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false))
        .count();
    Ok(count)
}

pub fn parse_peer_message_kind(s: &str) -> Option<PeerMessageKind> {
    match s {
        "observation" | "Observation" => Some(PeerMessageKind::Observation),
        "correction" | "Correction" => Some(PeerMessageKind::Correction),
        "question" | "Question" => Some(PeerMessageKind::Question),
        "acknowledgement" | "Acknowledgement" => Some(PeerMessageKind::Acknowledgement),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_msg(
        parent: &str,
        sender_agent: &str,
        kind: PeerMessageKind,
        recipient: &str,
        content: &str,
    ) -> PeerMessage {
        PeerMessage::new(
            format!("{parent}-{sender_agent}-test-uuid"),
            sender_agent,
            parent,
            recipient,
            kind,
            content,
        )
    }

    #[test]
    fn round_trip_single_message() {
        let dir = TempDir::new().unwrap();
        let msg = make_msg(
            "parent-1",
            "rust-fixer",
            PeerMessageKind::Observation,
            "*",
            "nonce check is load-bearing",
        );
        append_message(dir.path(), &msg).unwrap();
        let read = read_messages(dir.path(), "parent-1", 0, None).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].content, "nonce check is load-bearing");
        assert_eq!(read[0].kind, PeerMessageKind::Observation);
    }

    #[test]
    fn after_ms_cursor_excludes_earlier_messages() {
        let dir = TempDir::new().unwrap();
        let mut msg1 = make_msg("p", "a1", PeerMessageKind::Observation, "*", "first");
        msg1.sent_at = 1000;
        let mut msg2 = make_msg("p", "a2", PeerMessageKind::Correction, "*", "second");
        msg2.sent_at = 2000;
        append_message(dir.path(), &msg1).unwrap();
        append_message(dir.path(), &msg2).unwrap();

        let after_first = read_messages(dir.path(), "p", 1000, None).unwrap();
        assert_eq!(after_first.len(), 1);
        assert_eq!(after_first[0].content, "second");
    }

    #[test]
    fn recipient_filter_delivers_broadcast_and_targeted() {
        let dir = TempDir::new().unwrap();
        let broadcast = make_msg("p", "a1", PeerMessageKind::Observation, "*", "for all");
        let targeted = make_msg(
            "p",
            "a1",
            PeerMessageKind::Correction,
            "reviewer",
            "for reviewer only",
        );
        let other = make_msg(
            "p",
            "a1",
            PeerMessageKind::Observation,
            "fixer",
            "not for reviewer",
        );
        append_message(dir.path(), &broadcast).unwrap();
        append_message(dir.path(), &targeted).unwrap();
        append_message(dir.path(), &other).unwrap();

        let for_reviewer = read_messages(dir.path(), "p", 0, Some("reviewer")).unwrap();
        assert_eq!(for_reviewer.len(), 2, "should receive broadcast + targeted");
        assert!(
            for_reviewer
                .iter()
                .all(|m| m.recipient == "*" || m.recipient == "reviewer")
        );
    }

    #[test]
    fn channel_full_error_at_depth_cap() {
        let dir = TempDir::new().unwrap();
        for i in 0..MAX_CHANNEL_DEPTH {
            let mut msg = make_msg("p", "a1", PeerMessageKind::Observation, "*", "fill");
            msg.id = format!("id-{i}");
            append_message(dir.path(), &msg).unwrap();
        }
        let overflow = make_msg("p", "a1", PeerMessageKind::Observation, "*", "overflow");
        let err = append_message(dir.path(), &overflow).unwrap_err();
        assert!(
            err.to_string().contains("full"),
            "expected full error, got: {err}"
        );
    }

    #[test]
    fn read_returns_empty_when_no_channel_file() {
        let dir = TempDir::new().unwrap();
        let msgs = read_messages(dir.path(), "no-such-task", 0, None).unwrap();
        assert!(msgs.is_empty());
    }

    #[test]
    fn read_batch_is_capped_at_max() {
        let dir = TempDir::new().unwrap();
        for i in 0..MAX_CHANNEL_READ_BATCH + 10 {
            let mut msg = make_msg("p", "a1", PeerMessageKind::Observation, "*", "x");
            msg.id = format!("id-{i}");
            msg.sent_at = i as u64 + 1;
            append_message(dir.path(), &msg).unwrap();
        }
        let batch = read_messages(dir.path(), "p", 0, None).unwrap();
        assert_eq!(batch.len(), MAX_CHANNEL_READ_BATCH);
    }

    #[test]
    fn skips_malformed_lines_without_panicking() {
        let dir = TempDir::new().unwrap();
        let path = channel_path(dir.path(), "p");
        std::fs::write(
            &path,
            "not-json\n{\"id\":\"x\",\"sent_at\":1,\"sender_id\":\"s\",\"sender_agent_id\":\"a\",\"recipient\":\"*\",\"kind\":\"Observation\",\"content\":\"ok\",\"parent_task_id\":\"p\"}\n",
        )
        .unwrap();
        let msgs = read_messages(dir.path(), "p", 0, None).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "ok");
    }

    #[test]
    fn parse_peer_message_kind_handles_all_variants() {
        assert_eq!(
            parse_peer_message_kind("observation"),
            Some(PeerMessageKind::Observation)
        );
        assert_eq!(
            parse_peer_message_kind("Correction"),
            Some(PeerMessageKind::Correction)
        );
        assert_eq!(
            parse_peer_message_kind("question"),
            Some(PeerMessageKind::Question)
        );
        assert_eq!(
            parse_peer_message_kind("Acknowledgement"),
            Some(PeerMessageKind::Acknowledgement)
        );
        assert_eq!(parse_peer_message_kind("invalid"), None);
    }
}
