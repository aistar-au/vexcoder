use super::super::stream_block::ToolStatus;
use super::{
    ConversationManager, ConversationStreamUpdate, TurnToolPolicy, UndoCheckpoint, streaming::*,
    tools::*,
};
use crate::types::{ContentBlock, StreamChunkMetadata};
use anyhow::{Result, anyhow};
use futures::future::join_all;
use tokio::sync::mpsc;

pub(super) struct CompletedToolCall {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) input: serde_json::Value,
    pub(super) result: Result<String>,
}

pub(super) fn emit_server_metadata_update(
    metadata: Option<&StreamChunkMetadata>,
    stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
) {
    let Some(metadata) = metadata else {
        return;
    };
    if metadata.prompt_progress.is_none() && metadata.timings.is_none() {
        return;
    }
    emit_stream_update(
        stream_delta_tx,
        ConversationStreamUpdate::ServerMetadata(Box::new(metadata.clone())),
    );
}

impl ConversationManager {
    pub(super) async fn execute_parallel_tool_round(
        &mut self,
        blocks: &[ContentBlock],
        original_user_input: &str,
        tool_timeout: std::time::Duration,
        use_structured_blocks: bool,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) -> Vec<CompletedToolCall> {
        if use_structured_blocks {
            for block in blocks {
                if let ContentBlock::ToolUse { id, .. } = block {
                    self.set_tool_call_status(id, ToolStatus::Executing, stream_delta_tx);
                }
            }
        }

        // Capture undo snapshots before tools run (they may modify files).
        let mut undo_snapshots: std::collections::HashMap<String, UndoCheckpoint> =
            std::collections::HashMap::new();
        if self.undo_enabled {
            for block in blocks {
                if let ContentBlock::ToolUse {
                    id, name, input, ..
                } = block
                    && let Some(checkpoint) = self.capture_undo_snapshot(name, input)
                {
                    undo_snapshots.insert(id.clone(), checkpoint);
                }
            }
        }

        let manager = &*self;
        let executions = blocks
            .iter()
            .filter_map(|block| {
                let ContentBlock::ToolUse {
                    id, name, input, ..
                } = block
                else {
                    return None;
                };
                let original_user_input = original_user_input.to_string();
                Some(async move {
                    CompletedToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                        result: if let Some(message) =
                            missing_read_only_location_prompt(name, input)
                                .or_else(|| missing_mutating_location_prompt(name, input))
                                .or_else(|| {
                                    mutating_tool_read_only_conflict_prompt(
                                        &original_user_input,
                                        name,
                                    )
                                }) {
                            Err(anyhow!(message))
                        } else {
                            manager
                                .execute_tool_with_timeout_with_updates(
                                    name,
                                    input,
                                    tool_timeout,
                                    stream_delta_tx,
                                )
                                .await
                        },
                    }
                })
            })
            .collect::<Vec<_>>();

        let completed = join_all(executions).await;

        // Push undo checkpoints for tools that succeeded.
        for call in &completed {
            if call.result.is_ok()
                && let Some(cp) = undo_snapshots.remove(&call.id)
            {
                self.push_undo_checkpoint(cp);
            }
        }

        completed
    }

    pub async fn send_message(
        &mut self,
        content: String,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) -> Result<String> {
        self.send_message_with_policy(content, stream_delta_tx, TurnToolPolicy::Default)
            .await
    }
}

#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
