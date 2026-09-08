use anyhow::Result;
use std::path::Path;

use crate::runtime::TaskState;
use crate::runtime::task_state::peer_channel::{
    self, AppendMessageError, MAX_PEER_MESSAGE_BYTES, PeerMessage, parse_peer_message_kind,
};

use super::types::PeerChannelError;

#[tracing::instrument(skip_all, fields(parent_task_id = %parent_task_id, sender_id = %sender_id))]
pub fn facade_post_peer_message(
    working_dir: &Path,
    parent_task_id: &str,
    sender_id: &str,
    sender_agent_id: &str,
    recipient: &str,
    kind: &str,
    content: &str,
) -> std::result::Result<PeerMessage, PeerChannelError> {
    let kind = parse_peer_message_kind(kind).ok_or(PeerChannelError::InvalidKind)?;

    if content.len() > MAX_PEER_MESSAGE_BYTES {
        return Err(PeerChannelError::ContentTooLong);
    }

    let state_dir = TaskState::state_dir_from(working_dir);

    let parent_state = load_parent_task_state(working_dir, parent_task_id)?;
    let sender_task = parent_state
        .session_tasks
        .iter()
        .find(|task| task.id == sender_id)
        .ok_or(PeerChannelError::SenderNotInTask)?;

    if sender_task.agent_id != sender_agent_id {
        tracing::warn!(
            parent_task_id,
            sender_id,
            provided_sender_agent_id = sender_agent_id,
            expected_sender_agent_id = %sender_task.agent_id,
            "peer message sender_agent_id mismatch; using persisted session task agent id"
        );
    }

    let message = PeerMessage::new(
        sender_id,
        sender_task.agent_id.clone(),
        parent_task_id,
        recipient,
        kind,
        content,
    );

    match peer_channel::append_message(&state_dir, &message) {
        Ok(()) => {}
        Err(AppendMessageError::ChannelFull) => return Err(PeerChannelError::ChannelFull),
        Err(AppendMessageError::Internal(err)) => return Err(PeerChannelError::Internal(err)),
    }

    Ok(message)
}

#[tracing::instrument(skip_all, fields(parent_task_id = %parent_task_id, after_ms = after_ms))]
pub fn facade_read_peer_messages(
    working_dir: &Path,
    parent_task_id: &str,
    after_ms: u64,
    recipient_filter: Option<&str>,
) -> Result<Vec<PeerMessage>> {
    let state_dir = TaskState::state_dir_from(working_dir);
    peer_channel::read_messages(&state_dir, parent_task_id, after_ms, recipient_filter)
}

fn load_parent_task_state(
    working_dir: &Path,
    parent_task_id: &str,
) -> std::result::Result<TaskState, PeerChannelError> {
    for dir in TaskState::state_search_dirs_from(working_dir) {
        let state_path = dir.join(format!("{parent_task_id}.json"));
        if !state_path.is_file() {
            continue;
        }

        return TaskState::load(&dir, parent_task_id).map_err(PeerChannelError::Internal);
    }

    Err(PeerChannelError::ParentTaskNotFound)
}
