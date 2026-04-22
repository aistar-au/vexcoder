

use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

use crate::runtime::session_task::now_millis;


pub const MAX_PEER_MESSAGE_BYTES: usize = 4_096;

pub const MAX_CHANNEL_DEPTH: usize = 256;

pub const MAX_CHANNEL_READ_BATCH: usize = 64;


const MAX_CHANNEL_FILE_BYTES: u64 = 1_048_576;

const MAX_LINE_BYTES: usize = 8_192;

const CHANNEL_LOCK_SUFFIX: &str = ".channel.lock";

#[derive(Debug, thiserror::Error)]
pub enum AppendMessageError {
    #[error("channel_full")]
    ChannelFull,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}


fn channel_serialization_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn channel_lock_path(state_dir: &Path, parent_task_id: &str) -> PathBuf {
    state_dir.join(format!("{parent_task_id}{CHANNEL_LOCK_SUFFIX}"))
}

fn open_channel_lock_file(state_dir: &Path, parent_task_id: &str) -> Result<File> {
    crate::tools::operator::policy::assert_durable_access(state_dir)?;

    let lock_path = channel_lock_path(state_dir, parent_task_id);
    crate::tools::operator::policy::assert_durable_access(&lock_path)?;

    std::fs::create_dir_all(state_dir)
        .with_context(|| format!("failed to create state dir: {}", state_dir.display()))?;

    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("failed to open channel lock: {}", lock_path.display()))
}


fn with_channel_lock<T, E>(
    state_dir: &Path,
    parent_task_id: &str,
    operation: impl FnOnce() -> std::result::Result<T, E>,
) -> std::result::Result<T, E>
where
    E: From<anyhow::Error>,
{
    let _in_process_guard = channel_serialization_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let lock_path = channel_lock_path(state_dir, parent_task_id);
    let lock_file = open_channel_lock_file(state_dir, parent_task_id).map_err(E::from)?;
    lock_file
        .lock_exclusive()
        .with_context(|| format!("failed to acquire channel lock: {}", lock_path.display()))
        .map_err(E::from)?;

    operation()
    
}

fn with_channel_shared_lock<T>(
    state_dir: &Path,
    parent_task_id: &str,
    operation: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let lock_path = channel_lock_path(state_dir, parent_task_id);
    let lock_file = open_channel_lock_file(state_dir, parent_task_id)?;
    lock_file.lock_shared().with_context(|| {
        format!(
            "failed to acquire shared channel lock: {}",
            lock_path.display()
        )
    })?;

    operation()
}


#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PeerMessageKind {
    
    Observation,
    
    Correction,
    
    Question,
    
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


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMessage {
    
    pub id: String,
    
    pub sent_at: u64,
    
    pub sender_id: String,
    
    pub sender_agent_id: String,
    
    pub recipient: String,
    
    pub kind: PeerMessageKind,
    
    pub content: String,
    
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


pub fn channel_path(state_dir: &Path, parent_task_id: &str) -> PathBuf {
    state_dir.join(format!("{parent_task_id}.channel.jsonl"))
}


pub fn append_message(
    state_dir: &Path,
    message: &PeerMessage,
) -> std::result::Result<(), AppendMessageError> {
    with_channel_lock(state_dir, &message.parent_task_id, || {
        append_message_inner(state_dir, message)
    })
}


fn append_message_inner(
    state_dir: &Path,
    message: &PeerMessage,
) -> std::result::Result<(), AppendMessageError> {
    let path = channel_path(state_dir, &message.parent_task_id);
    crate::tools::operator::policy::assert_durable_access(&path)?;

    
    let existing = count_lines(&path)?;
    if existing >= MAX_CHANNEL_DEPTH {
        return Err(AppendMessageError::ChannelFull);
    }

    
    if let Ok(file_info) = std::fs::metadata(&path)
        && file_info.len() >= MAX_CHANNEL_FILE_BYTES
    {
        return Err(AppendMessageError::ChannelFull);
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


pub fn read_messages(
    state_dir: &Path,
    parent_task_id: &str,
    after_ms: u64,
    recipient_filter: Option<&str>,
) -> Result<Vec<PeerMessage>> {
    let path = channel_path(state_dir, parent_task_id);
    crate::tools::operator::policy::assert_durable_access(&path)?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    with_channel_shared_lock(state_dir, parent_task_id, || {
        let file = File::open(&path)
            .with_context(|| format!("failed to open channel file: {}", path.display()))?;

        let mut results = Vec::with_capacity(MAX_CHANNEL_READ_BATCH.min(32));
        let mut line_buf = String::new();
        let mut buf_reader = BufReader::new(&file);

        loop {
            line_buf.clear();
            let bytes_read = buf_reader
                .read_line(&mut line_buf)
                .with_context(|| format!("failed to read channel file line: {}", path.display()))?;
            if bytes_read == 0 {
                break;
            }

            
            if line_buf.len() > MAX_LINE_BYTES || line_buf.trim().is_empty() {
                continue;
            }

            let msg: PeerMessage = match serde_json::from_str(line_buf.trim()) {
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

            if let Some(agent_id) = recipient_filter
                && msg.recipient != "*"
                && msg.recipient != agent_id
            {
                continue;
            }

            results.push(msg);
            if results.len() >= MAX_CHANNEL_READ_BATCH {
                break;
            }
        }

        Ok(results)
    })
}


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
        assert!(matches!(err, AppendMessageError::ChannelFull));
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

    #[test]
    fn file_size_safety_valve_rejects_oversized_channel() {
        let dir = TempDir::new().unwrap();
        let path = channel_path(dir.path(), "p");
        
        std::fs::create_dir_all(dir.path()).unwrap();
        let big_content = "x".repeat(MAX_CHANNEL_FILE_BYTES as usize + 1);
        std::fs::write(&path, big_content).unwrap();

        let msg = make_msg("p", "a1", PeerMessageKind::Observation, "*", "rejected");
        let err = append_message(dir.path(), &msg).unwrap_err();
        assert!(matches!(err, AppendMessageError::ChannelFull));
    }

    #[test]
    fn oversized_line_skipped_during_read() {
        let dir = TempDir::new().unwrap();
        let path = channel_path(dir.path(), "p");
        
        let valid1 = serde_json::to_string(&PeerMessage {
            id: "m1".into(),
            sent_at: 1,
            sender_id: "s".into(),
            sender_agent_id: "a".into(),
            recipient: "*".into(),
            kind: PeerMessageKind::Observation,
            content: "first".into(),
            parent_task_id: "p".into(),
        })
        .unwrap();
        let oversized = "x".repeat(MAX_LINE_BYTES + 100);
        let valid2 = serde_json::to_string(&PeerMessage {
            id: "m2".into(),
            sent_at: 2,
            sender_id: "s".into(),
            sender_agent_id: "a".into(),
            recipient: "*".into(),
            kind: PeerMessageKind::Correction,
            content: "second".into(),
            parent_task_id: "p".into(),
        })
        .unwrap();
        std::fs::write(&path, format!("{valid1}\n{oversized}\n{valid2}\n")).unwrap();

        let msgs = read_messages(dir.path(), "p", 0, None).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "first");
        assert_eq!(msgs[1].content, "second");
    }
}
