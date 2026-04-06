use super::super::stream_block::{StreamBlock, ToolStatus};
use super::{ConversationManager, ConversationStreamUpdate};
use crate::runtime::json_handoff::RuntimeEvent;
use crate::runtime::task_document::{AssistantPhase, TurnEntry};
use std::collections::BTreeSet;
use tokio::sync::mpsc;

impl ConversationManager {
    /// Insert or update a stream block in the active turn and emit a
    /// `BlockStart` update to the TUI channel.
    ///
    /// For deferred text blocks (tx is None) only the reducer state is updated;
    /// the stream update is withheld until `flush_deferred_thinking_blocks` is
    /// called.
    pub(super) fn upsert_turn_block(
        &mut self,
        index: usize,
        block: StreamBlock,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) {
        // Emit collapsed Thinking placeholders for any gap between the last
        // emitted block index and the target index.  This keeps the TUI block
        // list contiguous even when the model skips indices.
        while self.current_round_stream_block_count < index {
            let pad_index = self.current_round_stream_block_count;
            let placeholder = StreamBlock::Thinking {
                content: String::new(),
                collapsed: true,
            };
            self.apply_doc_event(RuntimeEvent::TranscriptBlockStart {
                index: pad_index,
                block: placeholder.clone(),
            });
            self.current_round_stream_block_count += 1;
            emit_stream_update(
                stream_delta_tx,
                ConversationStreamUpdate::BlockStart {
                    index: pad_index,
                    block: placeholder,
                },
            );
        }
        self.current_round_stream_block_count = index + 1;

        // Apply the canonical block-start event to the reducer.
        let event = match &block {
            StreamBlock::Thinking { content, collapsed } => {
                Some(RuntimeEvent::TranscriptBlockStart {
                    index,
                    block: StreamBlock::Thinking {
                        content: content.clone(),
                        collapsed: *collapsed,
                    },
                })
            }
            StreamBlock::FinalText { content } => Some(RuntimeEvent::TranscriptBlockStart {
                index,
                block: StreamBlock::FinalText {
                    content: content.clone(),
                },
            }),
            // RuntimeEvent::TranscriptBlockStart ignores ToolCall blocks in
            // the reducer; use the dedicated ToolCall variant instead.
            StreamBlock::ToolCall {
                id, name, input, ..
            } => Some(RuntimeEvent::ToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: input.clone(),
            }),
            _ => None,
        };
        if let Some(ev) = event {
            self.apply_doc_event(ev);
        }

        emit_stream_update(
            stream_delta_tx,
            ConversationStreamUpdate::BlockStart { index, block },
        );
    }

    /// Append `text` to the Thinking block at `index`, computing only the
    /// incremental suffix relative to what is already stored.  Returns the
    /// appended suffix so the caller can forward it to the stream.
    pub(super) fn append_text_delta(
        &mut self,
        index: usize,
        text: &str,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) -> String {
        let mut appended = String::new();
        let mut found_entry = false;

        if let Some(doc) = self.task_doc.as_mut() {
            if let Some(active) = doc.active_turn.as_mut() {
                // Find the latest AssistantBlock entry with this block_index.
                if let Some(block_entry) = active.entries.iter_mut().rev().find_map(|entry| {
                    if let TurnEntry::AssistantBlock { block, .. } = entry {
                        if block.block_index == index {
                            return Some(block);
                        }
                    }
                    None
                }) {
                    found_entry = true;
                    appended = append_incremental_suffix(&mut block_entry.content, text);
                    if !appended.is_empty() && active.ttft_ms.is_none() {
                        use crate::runtime::session_task::now_millis;
                        active.ttft_ms = Some(now_millis().saturating_sub(active.started_at_ms));
                    }
                }
            }
        }

        if !found_entry {
            // No existing entry at this index; create one.
            appended = text.to_string();
            self.upsert_turn_block(
                index,
                StreamBlock::Thinking {
                    content: text.to_string(),
                    collapsed: false,
                },
                stream_delta_tx,
            );
            return appended;
        }

        if !appended.is_empty() {
            emit_stream_update(
                stream_delta_tx,
                ConversationStreamUpdate::BlockDelta {
                    index,
                    delta: appended.clone(),
                },
            );
        }

        appended
    }

    /// Update the status of a ToolCall entry and re-emit a BlockStart update.
    pub(super) fn set_tool_call_status(
        &mut self,
        tool_call_id: &str,
        status: ToolStatus,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) {
        let mut emit_info: Option<(usize, StreamBlock)> = None;

        if let Some(doc) = self.task_doc.as_mut() {
            if let Some(active) = doc.active_turn.as_mut() {
                for (entry_idx, entry) in active.entries.iter_mut().enumerate() {
                    if let TurnEntry::ToolCall {
                        id,
                        name,
                        input,
                        status: current_status,
                        ..
                    } = entry
                    {
                        if id == tool_call_id {
                            *current_status = status;
                            emit_info = Some((
                                entry_idx,
                                StreamBlock::ToolCall {
                                    id: id.clone(),
                                    name: name.clone(),
                                    input: input.clone(),
                                    status: current_status.clone(),
                                },
                            ));
                            break;
                        }
                    }
                }
            }
        }

        if let Some((index, block)) = emit_info {
            emit_stream_update(
                stream_delta_tx,
                ConversationStreamUpdate::BlockStart { index, block },
            );
        }
    }

    /// Push a ToolResult entry into the active turn and emit BlockStart +
    /// BlockComplete updates.
    pub(super) fn push_tool_result_block(
        &mut self,
        block: StreamBlock,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) {
        if let StreamBlock::ToolResult {
            ref tool_call_id,
            ref output,
            is_error,
        } = block
        {
            let tool_name = self
                .task_doc
                .as_ref()
                .and_then(|d| d.active_turn.as_ref())
                .and_then(|a| {
                    a.entries.iter().rev().find_map(|e| {
                        if let TurnEntry::ToolCall { id, name, .. } = e {
                            if id == tool_call_id {
                                return Some(name.clone());
                            }
                        }
                        None
                    })
                });

            self.apply_doc_event(RuntimeEvent::ToolResult {
                tool_call_id: tool_call_id.clone(),
                tool_name,
                is_error,
                output: output.clone(),
            });
        }

        let index = self
            .task_doc
            .as_ref()
            .and_then(|d| d.active_turn.as_ref())
            .map(|a| a.entries.len().saturating_sub(1))
            .unwrap_or(0);

        emit_stream_update(
            stream_delta_tx,
            ConversationStreamUpdate::BlockStart {
                index,
                block: block.clone(),
            },
        );
        emit_stream_update(
            stream_delta_tx,
            ConversationStreamUpdate::BlockComplete { index },
        );
    }

    /// Emit deferred Thinking blocks whose stream updates were withheld.
    pub(super) fn flush_deferred_thinking_blocks(
        &self,
        deferred_text_block_indices: &mut BTreeSet<usize>,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) {
        let pending_indices: Vec<usize> = deferred_text_block_indices.iter().copied().collect();

        for index in pending_indices {
            let Some(doc) = self.task_doc.as_ref() else {
                continue;
            };
            let Some(active) = doc.active_turn.as_ref() else {
                continue;
            };
            let Some(block_entry) = active.entries.iter().rev().find_map(|e| {
                if let TurnEntry::AssistantBlock { block, .. } = e {
                    if block.block_index == index {
                        return Some(block);
                    }
                }
                None
            }) else {
                continue;
            };

            emit_stream_update(
                stream_delta_tx,
                ConversationStreamUpdate::BlockStart {
                    index,
                    block: StreamBlock::Thinking {
                        content: String::new(),
                        collapsed: block_entry.collapsed,
                    },
                },
            );
            if !block_entry.content.is_empty() {
                emit_stream_update(
                    stream_delta_tx,
                    ConversationStreamUpdate::BlockDelta {
                        index,
                        delta: block_entry.content.clone(),
                    },
                );
            }
            deferred_text_block_indices.remove(&index);
        }
    }

    /// At the end of the final API round, change remaining Thinking entries
    /// from this round to FinalText and emit stream updates.
    pub(super) fn promote_thinking_blocks_to_final_text(
        &mut self,
        deferred_text_block_indices: &BTreeSet<usize>,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) {
        let round_start = self.current_round_entry_start;

        let Some(doc) = self.task_doc.as_mut() else {
            return;
        };
        let Some(active) = doc.active_turn.as_mut() else {
            return;
        };

        for entry in active.entries[round_start..].iter_mut() {
            if let TurnEntry::AssistantBlock { block, .. } = entry {
                if block.phase == AssistantPhase::Thinking {
                    let full_content = block.content.clone();
                    block.phase = AssistantPhase::Final;
                    block.streaming = false;

                    // If the block was deferred, include the full content in
                    // the FinalText emit.  Otherwise the content was already
                    // streamed as Thinking deltas so emit empty content.
                    let emit_content = if deferred_text_block_indices.contains(&block.block_index) {
                        full_content
                    } else {
                        String::new()
                    };

                    emit_stream_update(
                        stream_delta_tx,
                        ConversationStreamUpdate::BlockStart {
                            index: block.block_index,
                            block: StreamBlock::FinalText {
                                content: emit_content,
                            },
                        },
                    );
                }
            }
        }
    }
}

pub(super) fn emit_stream_update(
    stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    update: ConversationStreamUpdate,
) {
    if let Some(tx) = stream_delta_tx {
        let _ = tx.send(update);
    }
}

pub(super) fn emit_text_update(
    stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    text: String,
) {
    emit_stream_update(stream_delta_tx, ConversationStreamUpdate::Delta(text));
}

pub(super) fn append_incremental_suffix(existing: &mut String, incoming: &str) -> String {
    let suffix = crate::state::transcript_delta::bounded_incremental_suffix(existing, incoming);
    existing.push_str(&suffix);
    suffix
}
