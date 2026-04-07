use super::*;
use crate::runtime::{AssistantPhase, NoticeSeverity, TurnEntry};
use crate::state::StreamBlock;

fn compact_preview_text(text: &str) -> String {
    const MAX_SUMMARY_WIDTH: usize = 60;

    let compact = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if compact.is_empty() || compact.len() <= MAX_SUMMARY_WIDTH {
        return compact;
    }

    let mut end = compact.floor_char_boundary(MAX_SUMMARY_WIDTH);
    if let Some(space_pos) = compact[..end].rfind(' ') {
        if space_pos > MAX_SUMMARY_WIDTH / 2 {
            end = space_pos;
        }
    }
    format!("{}\u{2026}", &compact[..end])
}

impl TuiMode {
    /// Allocate the next monotonic step ID from `task_doc.meta`.
    fn alloc_step_id(&mut self) -> u64 {
        let id = self.task_doc.meta.next_step_id;
        self.task_doc.meta.next_step_id = self.task_doc.meta.next_step_id.saturating_add(1);
        id
    }

    /// Append a `TurnEntry` to the active turn's entry list.
    fn append_turn_entry(&mut self, entry: TurnEntry) {
        if let Some(active) = self.task_doc.active_turn.as_mut() {
            active.entries.push(entry);
        }
    }

    /// Push a system-notice into the active turn (or pre-session list if no
    /// turn is open).
    pub(super) fn push_document_notice(&mut self, message: String, severity: NoticeSeverity) {
        if self.task_doc.active_turn.is_some() {
            let step_id = self.alloc_step_id();
            self.append_turn_entry(TurnEntry::SystemNotice {
                step_id,
                message,
                severity,
            });
        } else {
            self.pre_session_notices.push(message);
        }
    }

    pub(super) fn on_model_update(&mut self, update: UiUpdate, ctx: &mut RuntimeContext) {
        match update {
            UiUpdate::TranscriptLine(line) => {
                let previous_output_len = self.expanded_output_row_count();
                if self.task_doc.active_turn.is_some() {
                    self.task_doc.meta.status = TaskStatus::Running;
                }
                const MAX_TRANSCRIPT_LINE_CHARS: usize = 512;
                let clipped = if line.len() > MAX_TRANSCRIPT_LINE_CHARS {
                    format!(
                        "{}... (+{} more chars omitted)",
                        &line[..MAX_TRANSCRIPT_LINE_CHARS],
                        line.len() - MAX_TRANSCRIPT_LINE_CHARS
                    )
                } else {
                    line
                };
                self.push_document_notice(clipped, NoticeSeverity::Info);
                self.preserve_transcript_scroll_on_growth(previous_output_len);
            }

            UiUpdate::StreamDelta(text) => {
                let previous_output_len = self.expanded_output_row_count();
                if self
                    .task_doc
                    .active_turn
                    .as_ref()
                    .is_some_and(|t| t.cancel_pending)
                {
                    return;
                }
                // When the protocol also sends StreamBlockDelta for Final
                // content, the block-level entries already contain the text.
                // Skip the flat delta to avoid double-counting.
                if self.stream_uses_block_deltas {
                    return;
                }
                if self.task_doc.active_turn.is_some() {
                    self.task_doc.meta.status = TaskStatus::Running;
                }
                if self.ttft.is_none() {
                    if let Some(started) = self.turn_started_at {
                        self.ttft = Some(started.elapsed());
                        if let Some(active) = self.task_doc.active_turn.as_mut() {
                            active.ttft_ms =
                                Some(started.elapsed().as_millis().try_into().unwrap_or(u64::MAX));
                        }
                    }
                }
                let appended = if let Some(active) = self.task_doc.active_turn.as_mut() {
                    active.entries.iter_mut().rev().any(|e| {
                        if let TurnEntry::AssistantBlock { block, .. } = e {
                            if block.streaming && block.phase == AssistantPhase::Final {
                                block.content.push_str(&text);
                                return true;
                            }
                        }
                        false
                    })
                } else {
                    false
                };
                if !appended && self.task_doc.active_turn.is_some() {
                    let step_id = self.alloc_step_id();
                    self.append_turn_entry(TurnEntry::AssistantBlock {
                        step_id,
                        block: crate::runtime::AssistantBlockEntry {
                            block_index: usize::MAX,
                            phase: AssistantPhase::Final,
                            content: text,
                            collapsed: false,
                            streaming: true,
                        },
                    });
                }
                self.clamp_transcript_after_mutation();
                self.preserve_transcript_scroll_on_growth(previous_output_len);
            }

            UiUpdate::ServerMetadata(metadata) => {
                let metadata = *metadata;
                if self
                    .task_doc
                    .active_turn
                    .as_ref()
                    .is_some_and(|t| t.cancel_pending)
                {
                    return;
                }
                if let Some(active) = self.task_doc.active_turn.as_mut() {
                    if let Some(progress) = metadata.prompt_progress {
                        active.prompt_progress = Some(progress);
                    }
                    if let Some(timings) = metadata.timings {
                        active.timings = Some(timings);
                    }
                }
            }

            UiUpdate::StreamBlockStart { index, block } => {
                if self.task_doc.active_turn.is_some() {
                    self.task_doc.meta.status = TaskStatus::Running;
                }
                let previous_output_len = self.expanded_output_row_count();
                match block {
                    StreamBlock::FinalText { content } => {
                        if self.ttft.is_none() {
                            if let Some(started) = self.turn_started_at {
                                self.ttft = Some(started.elapsed());
                                if let Some(active) = self.task_doc.active_turn.as_mut() {
                                    active.ttft_ms = Some(
                                        started
                                            .elapsed()
                                            .as_millis()
                                            .try_into()
                                            .unwrap_or(u64::MAX),
                                    );
                                }
                            }
                        }
                        let step_id = self.alloc_step_id();
                        self.append_turn_entry(TurnEntry::AssistantBlock {
                            step_id,
                            block: crate::runtime::AssistantBlockEntry {
                                block_index: index,
                                phase: AssistantPhase::Final,
                                content,
                                collapsed: false,
                                streaming: true,
                            },
                        });
                    }
                    StreamBlock::Thinking { content, collapsed } => {
                        let step_id = self.alloc_step_id();
                        self.append_turn_entry(TurnEntry::AssistantBlock {
                            step_id,
                            block: crate::runtime::AssistantBlockEntry {
                                block_index: index,
                                phase: AssistantPhase::Thinking,
                                content,
                                collapsed,
                                streaming: true,
                            },
                        });
                    }
                    StreamBlock::ToolCall {
                        id, name, input, ..
                    } => {
                        let step_id = self.alloc_step_id();
                        self.append_turn_entry(TurnEntry::ToolCall {
                            step_id,
                            id,
                            name,
                            input,
                            status: crate::state::ToolStatus::Pending,
                        });
                        self.streaming_tool_input_buffers
                            .insert(index, String::new());
                        if self.timeline_follow_mode {
                            let total = self.timeline_entry_count();
                            self.selected_timeline_index = total.saturating_sub(1);
                            self.inspector_scroll_offset = 0;
                        }
                    }
                    StreamBlock::ToolResult {
                        tool_call_id,
                        output,
                        is_error,
                    } => {
                        // Update matching ToolCall status and record changed files.
                        if let Some(active) = self.task_doc.active_turn.as_mut() {
                            for entry in &mut active.entries {
                                if let TurnEntry::ToolCall { id, status, .. } = entry {
                                    if *id == tool_call_id {
                                        *status = if is_error {
                                            crate::state::ToolStatus::Error
                                        } else {
                                            crate::state::ToolStatus::Complete
                                        };
                                        break;
                                    }
                                }
                            }
                        }
                        if !is_error {
                            let name_and_input = self.task_doc.active_turn.as_ref().and_then(|a| {
                                a.entries.iter().rev().find_map(|e| {
                                    if let TurnEntry::ToolCall {
                                        id, name, input, ..
                                    } = e
                                    {
                                        if *id == tool_call_id {
                                            return Some((name.clone(), input.clone()));
                                        }
                                    }
                                    None
                                })
                            });
                            if let Some((name, input)) = name_and_input {
                                let mut changed = std::collections::BTreeSet::new();
                                note_changed_files_from_tool_call(&mut changed, &name, &input);
                                if !changed.is_empty() {
                                    self.invalidate_file_prompt_entries();
                                    if let Some(active) = self.task_doc.active_turn.as_mut() {
                                        active.changed_files.extend(changed);
                                    }
                                }
                            }
                        }
                        let step_id = self.alloc_step_id();
                        self.append_turn_entry(TurnEntry::ToolResult {
                            step_id,
                            tool_call_id,
                            tool_name: None,
                            output,
                            is_error,
                        });
                    }
                }
                self.clamp_transcript_after_mutation();
                self.preserve_transcript_scroll_on_growth(previous_output_len);
            }

            UiUpdate::StreamBlockDelta { index, delta } => {
                let updated_text = if let Some(active) = self.task_doc.active_turn.as_mut() {
                    active.entries.iter_mut().rev().any(|e| {
                        if let TurnEntry::AssistantBlock { block, .. } = e {
                            if block.block_index == index {
                                block.content.push_str(&delta);
                                self.stream_uses_block_deltas = true;
                                return true;
                            }
                        }
                        false
                    })
                } else {
                    false
                };
                if !updated_text {
                    if let Some(raw) = self.streaming_tool_input_buffers.get_mut(&index) {
                        raw.push_str(&delta);
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw) {
                            if let Some(active) = self.task_doc.active_turn.as_mut() {
                                // Update the last ToolCall entry's input.
                                for entry in active.entries.iter_mut().rev() {
                                    if let TurnEntry::ToolCall { input, .. } = entry {
                                        *input = parsed;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                self.clamp_transcript_after_mutation();
            }

            UiUpdate::StreamBlockComplete { index } => {
                if let Some(active) = self.task_doc.active_turn.as_mut() {
                    for entry in active.entries.iter_mut().rev() {
                        if let TurnEntry::AssistantBlock { block, .. } = entry {
                            if block.block_index == index {
                                block.streaming = false;
                                break;
                            }
                        }
                    }
                }
                self.streaming_tool_input_buffers.remove(&index);
            }

            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name,
                input_preview,
                response_tx,
            }) => {
                if self
                    .task_doc
                    .active_turn
                    .as_ref()
                    .is_some_and(|t| t.cancel_pending)
                {
                    let _ = response_tx.send(false);
                    return;
                }
                self.resolve_pending_approval(false, ctx);
                self.resolve_pending_patch_approval(false);
                if self.read_only_turn_active {
                    let _ = response_tx.send(false);
                    return;
                }
                if self.overlay_state.auto_approve_session {
                    let step_id = self.pending_tool_step_id(&tool_name, &input_preview);
                    self.mark_tool_step_approved(step_id);
                    let _ = response_tx.send(true);
                    self.push_document_notice(
                        format!("[approval] {tool_name} auto-approved for session"),
                        NoticeSeverity::Info,
                    );
                    return;
                }
                if let Some((capability, scope)) =
                    capability_for_tool_name(&tool_name).and_then(|cap| {
                        self.task_doc
                            .meta
                            .active_grants
                            .get(&cap)
                            .copied()
                            .map(|scope| (cap, scope))
                    })
                {
                    if matches!(scope, ApprovalScope::Once) {
                        self.task_doc.meta.active_grants.remove(&capability);
                    }
                    let step_id = self.pending_tool_step_id(&tool_name, &input_preview);
                    self.mark_tool_step_approved(step_id);
                    let _ = response_tx.send(true);
                    self.push_document_notice(
                        format!(
                            "[approval] {tool_name} auto-approved via {} grant",
                            scope_to_label(scope)
                        ),
                        NoticeSeverity::Info,
                    );
                    return;
                }
                let summary = summarize_tool_approval_context(&tool_name, &input_preview);
                let step_id = self.pending_tool_step_id(&tool_name, &input_preview);
                let previous_output_len = self.expanded_output_row_count();
                self.push_document_notice(
                    format!("[approval] {summary} · awaiting approval"),
                    NoticeSeverity::Info,
                );
                {
                    let compact = compact_preview_text(&input_preview);
                    if !compact.is_empty() {
                        self.push_document_notice(
                            format!("[approval_detail] Input: {compact}"),
                            NoticeSeverity::Info,
                        );
                    }
                }
                self.preserve_transcript_scroll_on_growth(previous_output_len);
                self.overlay_state.pending_approval = Some(PendingApproval {
                    step_id,
                    tool_name,
                    input_preview,
                    action: PendingApprovalAction::Tool(response_tx),
                });
                self.set_task_status(TaskStatus::AwaitingApproval);
            }

            UiUpdate::EditLoopComplete {
                outcome,
                last_validation_result,
            } => {
                if let Some(t) = self.task_doc.active_turn.as_mut() {
                    t.command_sessions.clear();
                }
                self.last_error_message = None;
                if let Some(result) = last_validation_result {
                    if let Some(edit_loop) = self.active_edit_loop.as_mut() {
                        edit_loop.set_last_validation_result(result);
                    }
                }
                self.resolve_pending_approval(false, ctx);
                self.resolve_pending_patch_approval(false);
                self.streaming_tool_input_buffers.clear();
                if let Some(active) = self.task_doc.active_turn.as_mut() {
                    active.cancel_pending = false;
                }
                match &outcome {
                    EditLoopOutcome::Success {
                        patch_applied,
                        validate_passed,
                    } => {
                        self.push_document_notice(
                            format!("[edit loop complete: patch_applied={patch_applied} validate_passed={validate_passed}]"),
                            NoticeSeverity::Info,
                        );
                    }
                    EditLoopOutcome::MaxTurnsReached { last_error } => {
                        let msg = last_error
                            .as_ref()
                            .map(|e| format!("[edit loop reached max turns — last error: {e}]"))
                            .unwrap_or_else(|| "[edit loop reached max turns]".to_string());
                        self.push_document_notice(msg, NoticeSeverity::Warning);
                    }
                    EditLoopOutcome::ApprovalDenied => {
                        self.push_document_notice(
                            "[edit loop aborted: approval denied]".to_string(),
                            NoticeSeverity::Warning,
                        );
                    }
                    EditLoopOutcome::Cancelled => {
                        self.push_document_notice(
                            "[edit loop cancelled]".to_string(),
                            NoticeSeverity::Info,
                        );
                    }
                }
                self.last_turn_duration = self.turn_started_at.map(|s| s.elapsed());
                self.last_turn_ttft = self.ttft;
                self.append_turn_timing_line();
                // Finish the active turn with the correct outcome.
                // finish_turn sets meta.status from the outcome, then we
                // override to Completed for Success (reducer maps
                // TurnOutcome::Completed → Ready, but edit-loop success
                // means the task is done).
                if self.task_doc.active_turn.is_some() {
                    let turn_outcome = match &outcome {
                        EditLoopOutcome::Success { .. } => TurnOutcome::Completed,
                        EditLoopOutcome::MaxTurnsReached { .. } => TurnOutcome::MaxTurnsReached,
                        EditLoopOutcome::ApprovalDenied => TurnOutcome::Cancelled,
                        EditLoopOutcome::Cancelled => TurnOutcome::Cancelled,
                    };
                    let turn_tokens = ctx.session_tokens_rollup().last_turn();
                    self.task_doc_reducer.finish_turn(
                        &mut self.task_doc,
                        turn_outcome,
                        turn_tokens,
                        crate::runtime::session_task::now_millis(),
                    );
                    if matches!(outcome, EditLoopOutcome::Success { .. }) {
                        self.task_doc.meta.status = TaskStatus::Completed;
                    }
                    self.persist_task_document();
                    self.reset_turn_capture();
                }
                self.clamp_transcript_after_mutation();
                self.transcript_scroll_offset = 0;
                self.inspector_scroll_offset = 0;
                self.read_only_turn_active = false;
                self.turn_completion_pending = false;
            }

            UiUpdate::CommandSessionStarted {
                session_id,
                command,
            } => {
                self.begin_command_session_with_id(session_id, command);
            }

            UiUpdate::CommandSessionAttached { session_id, pid } => {
                if let Some(session) = self
                    .task_doc
                    .active_turn
                    .as_mut()
                    .and_then(|t| t.command_sessions.get_mut(&session_id))
                {
                    session.pid = pid;
                }
            }

            UiUpdate::CommandSessionFinished { session_id } => {
                if let Some(active) = self.task_doc.active_turn.as_mut() {
                    active.command_sessions.remove(&session_id);
                }
                self.complete_turn_if_idle(ctx);
            }

            UiUpdate::TurnComplete => {
                self.turn_completion_pending = true;
                self.complete_turn_if_idle(ctx);
            }

            UiUpdate::ContextCompacted {
                messages_before,
                messages_after,
                summary,
            } => {
                use crate::runtime::ContextCompactionRecord;
                let turn_index = self.task_doc.completed_turns.len();
                self.task_doc
                    .context_compaction
                    .push(ContextCompactionRecord {
                        turn_index,
                        messages_before,
                        messages_after,
                        summary,
                    });
            }

            UiUpdate::Error(msg) => {
                if let Some(t) = self.task_doc.active_turn.as_mut() {
                    t.command_sessions.clear();
                }
                self.resolve_pending_approval(false, ctx);
                self.resolve_pending_patch_approval(false);
                self.streaming_tool_input_buffers.clear();
                self.last_turn_duration = self.turn_started_at.map(|s| s.elapsed());
                self.last_error_message = Some(msg.clone());
                self.push_document_notice(format!("[error] {msg}"), NoticeSeverity::Error);
                if let Some(active) = self.task_doc.active_turn.as_mut() {
                    active.cancel_pending = false;
                }
                self.read_only_turn_active = false;
                self.transcript_scroll_offset = 0;
                self.inspector_scroll_offset = 0;
                let tokens = ctx.session_tokens_rollup().last_turn();
                self.task_doc_reducer.finish_turn(
                    &mut self.task_doc,
                    TurnOutcome::Failed { message: msg },
                    tokens,
                    crate::runtime::session_task::now_millis(),
                );
                self.set_task_status(TaskStatus::Failed);
            }
        }
    }
}
