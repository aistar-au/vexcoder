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
    /// For deferred text blocks (tx is None) only the condenser state is updated;
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

        // Apply the canonical block-start event to the condenser.
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
            // the condenser; use the dedicated ToolCall variant instead.
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
    ///
    /// ADR-045 Invariant A: all writes to `task_doc` go through the condenser
    /// via `apply_doc_event`.  We compute the deduplicated delta with a
    /// read-only pass, then emit `TranscriptBlockDelta` so the condenser
    /// performs the actual write.
    pub(super) fn append_text_delta(
        &mut self,
        index: usize,
        text: &str,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) -> String {
        // Read-only pass: find the block and compute the deduplicated delta.
        let (delta, found_entry) = {
            let doc_opt = self.task_doc.as_ref();
            match doc_opt.and_then(|d| d.active_turn.as_ref()) {
                Some(active) => {
                    let maybe_block = active.entries.iter().rev().find_map(|entry| {
                        if let TurnEntry::AssistantBlock { block, .. } = entry {
                            if block.block_index == index {
                                return Some(block);
                            }
                        }
                        None
                    });
                    match maybe_block {
                        Some(block_entry) => {
                            let d = crate::state::transcript_delta::bounded_incremental_suffix(
                                &block_entry.content,
                                text,
                            );
                            (d, true)
                        }
                        None => (String::new(), false),
                    }
                }
                None => (String::new(), false),
            }
        };

        if !found_entry {
            // No existing entry at this index; create one via the condenser.
            self.upsert_turn_block(
                index,
                StreamBlock::Thinking {
                    content: text.to_string(),
                    collapsed: false,
                },
                stream_delta_tx,
            );
            return text.to_string();
        }

        if delta.is_empty() {
            return String::new();
        }

        // Write via condenser (ADR-045 sole-writer).
        self.apply_doc_event(RuntimeEvent::TranscriptBlockDelta {
            index,
            delta: delta.clone(),
        });

        emit_stream_update(
            stream_delta_tx,
            ConversationStreamUpdate::BlockDelta {
                index,
                delta: delta.clone(),
            },
        );

        delta
    }

    /// Update the status of a ToolCall entry and re-emit a BlockStart update.
    /// Update the status of a ToolCall entry and re-emit a BlockStart update.
    ///
    /// ADR-045 Invariant A: the status mutation is routed through the condenser
    /// via `apply_doc_event(RuntimeEvent::ToolCallStatusUpdated)` so the
    /// document has a single write path.  A read-only pass collects the
    /// entry index and block shape needed for the stream update.
    pub(super) fn set_tool_call_status(
        &mut self,
        tool_call_id: &str,
        status: ToolStatus,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) {
        // Read-only pass: find the entry index and pre-update block shape.
        let emit_info: Option<(usize, StreamBlock)> = {
            self.task_doc
                .as_ref()
                .and_then(|d| d.active_turn.as_ref())
                .and_then(|active| {
                    active
                        .entries
                        .iter()
                        .enumerate()
                        .find_map(|(entry_idx, entry)| {
                            if let TurnEntry::ToolCall {
                                id, name, input, ..
                            } = entry
                            {
                                if id == tool_call_id {
                                    return Some((
                                        entry_idx,
                                        StreamBlock::ToolCall {
                                            id: id.clone(),
                                            name: name.clone(),
                                            input: input.clone(),
                                            status: status.clone(),
                                        },
                                    ));
                                }
                            }
                            None
                        })
                })
        };

        if let Some((index, block)) = emit_info {
            // Write via condenser (ADR-045 sole-writer).
            self.apply_doc_event(RuntimeEvent::ToolCallStatusUpdated {
                tool_call_id: tool_call_id.to_string(),
                status,
            });
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
    ///
    /// ADR-045 Invariant A: phase mutations are routed through the condenser
    /// via `apply_doc_event(RuntimeEvent::TranscriptBlockPhaseUpdated)` so
    /// the document has a single write path.  A read-only pass collects the
    /// block indices and content needed for the stream updates.
    pub(super) fn promote_thinking_blocks_to_final_text(
        &mut self,
        deferred_text_block_indices: &BTreeSet<usize>,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    ) {
        let round_start = self.current_round_entry_start;

        // Read-only pass: collect block index + content for each Thinking block.
        let promotions: Vec<(usize, String)> = {
            let Some(doc) = self.task_doc.as_ref() else {
                return;
            };
            let Some(active) = doc.active_turn.as_ref() else {
                return;
            };
            active.entries[round_start..]
                .iter()
                .filter_map(|entry| {
                    if let TurnEntry::AssistantBlock { block, .. } = entry {
                        if block.phase == AssistantPhase::Thinking {
                            let emit_content =
                                if deferred_text_block_indices.contains(&block.block_index) {
                                    block.content.clone()
                                } else {
                                    String::new()
                                };
                            return Some((block.block_index, emit_content));
                        }
                    }
                    None
                })
                .collect()
        };

        // Write via condenser + emit stream updates (ADR-045 sole-writer).
        for (block_index, emit_content) in promotions {
            self.apply_doc_event(RuntimeEvent::TranscriptBlockPhaseUpdated {
                index: block_index,
                phase: AssistantPhase::Final,
                streaming: false,
            });

            emit_stream_update(
                stream_delta_tx,
                ConversationStreamUpdate::BlockStart {
                    index: block_index,
                    block: StreamBlock::FinalText {
                        content: emit_content,
                    },
                },
            );
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

#[cfg(test)]
pub(super) fn append_incremental_suffix(existing: &mut String, incoming: &str) -> String {
    let suffix = crate::state::transcript_delta::bounded_incremental_suffix(existing, incoming);
    existing.push_str(&suffix);
    suffix
}
