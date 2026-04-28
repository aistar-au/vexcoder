use super::*;
use crate::runtime::{AssistantPhase, NoticeSeverity, PulseEntry};
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
    if let Some(space_pos) = compact[..end].rfind(' ')
        && space_pos > MAX_SUMMARY_WIDTH / 2
    {
        end = space_pos;
    }
    format!("{}\u{2026}", &compact[..end])
}

impl TuiMode {
    fn scrub_materialized_tool_markup(&mut self) {
        let Some(active) = self.task_doc.active_pulse.as_mut() else {
            return;
        };

        for entry in &mut active.entries {
            if let PulseEntry::AssistantBlock { block, .. } = entry
                && block.phase == AssistantPhase::Final
            {
                block.content = crate::runtime::policy::sanitize_assistant_text(&block.content);
            }
        }

        active.entries.retain(|entry| {
            !matches!(
                entry,
                PulseEntry::AssistantBlock { block, .. }
                    if block.phase == AssistantPhase::Final && block.content.trim().is_empty()
            )
        });
    }

    fn alloc_step_id(&mut self) -> u64 {
        let id = self.task_doc.info.next_step_id;
        self.task_doc.info.next_step_id = self.task_doc.info.next_step_id.saturating_add(1);
        id
    }

    fn append_turn_entry(&mut self, entry: PulseEntry) {
        if let Some(active) = self.task_doc.active_pulse.as_mut() {
            active.entries.push(entry);
        }
    }

    pub(super) fn push_document_notice(&mut self, message: String, severity: NoticeSeverity) {
        if self.task_doc.active_pulse.is_some() {
            let step_id = self.alloc_step_id();
            self.append_turn_entry(PulseEntry::SystemNotice {
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
                if self.task_doc.active_pulse.is_some() {
                    self.task_doc.info.status = TaskStatus::Running;
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
                    .active_pulse
                    .as_ref()
                    .is_some_and(|t| t.cancel_pending)
                {
                    return;
                }

                if self.stream_uses_structured_final_output {
                    return;
                }
                if self.task_doc.active_pulse.is_some() {
                    self.task_doc.info.status = TaskStatus::Running;
                }
                if self.ttft.is_none()
                    && let Some(started) = self.turn_started_at
                {
                    self.ttft = Some(started.elapsed());
                    if let Some(active) = self.task_doc.active_pulse.as_mut() {
                        active.ttft_ms =
                            Some(started.elapsed().as_millis().try_into().unwrap_or(u64::MAX));
                    }
                }
                let appended = if let Some(active) = self.task_doc.active_pulse.as_mut() {
                    active.entries.iter_mut().rev().any(|e| {
                        if let PulseEntry::AssistantBlock { block, .. } = e
                            && block.streaming
                            && block.phase == AssistantPhase::Final
                        {
                            block.content.push_str(&text);
                            return true;
                        }
                        false
                    })
                } else {
                    false
                };
                if !appended && self.task_doc.active_pulse.is_some() {
                    let step_id = self.alloc_step_id();
                    self.append_turn_entry(PulseEntry::AssistantBlock {
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
                self.scrub_materialized_tool_markup();
                self.clamp_transcript_after_mutation();
                self.preserve_transcript_scroll_on_growth(previous_output_len);
            }

            UiUpdate::ServerMetadata(metadata) => {
                let metadata = *metadata;
                if self
                    .task_doc
                    .active_pulse
                    .as_ref()
                    .is_some_and(|t| t.cancel_pending)
                {
                    return;
                }
                if let Some(active) = self.task_doc.active_pulse.as_mut() {
                    if let Some(progress) = metadata.prompt_progress {
                        active.prompt_progress = Some(progress);
                    }
                    if let Some(timings) = metadata.timings {
                        active.timings = Some(timings);
                    }
                }
            }

            UiUpdate::StreamBlockStart { index, block } => {
                if self.task_doc.active_pulse.is_some() {
                    self.task_doc.info.status = TaskStatus::Running;
                }
                let previous_output_len = self.expanded_output_row_count();
                match block {
                    StreamBlock::FinalText { content } => {
                        self.stream_uses_structured_final_output = true;
                        if self.ttft.is_none()
                            && let Some(started) = self.turn_started_at
                        {
                            self.ttft = Some(started.elapsed());
                            if let Some(active) = self.task_doc.active_pulse.as_mut() {
                                active.ttft_ms = Some(
                                    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
                                );
                            }
                            self.scrub_materialized_tool_markup();
                        }
                        let updated_existing =
                            if let Some(active) = self.task_doc.active_pulse.as_mut() {
                                if let Some(PulseEntry::AssistantBlock { block, .. }) =
                                    active.entries.last_mut()
                                {
                                    if block.block_index == index
                                        || (block.block_index == usize::MAX
                                            && block.phase == AssistantPhase::Final)
                                    {
                                        block.phase = AssistantPhase::Final;
                                        block.block_index = index;
                                        block.collapsed = false;
                                        block.streaming = true;
                                        if !content.is_empty() {
                                            block.content = content.clone();
                                        }
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                        if updated_existing {
                            self.clamp_transcript_after_mutation();
                            self.preserve_transcript_scroll_on_growth(previous_output_len);
                            return;
                        }
                        let step_id = self.alloc_step_id();
                        self.append_turn_entry(PulseEntry::AssistantBlock {
                            step_id,
                            block: crate::runtime::AssistantBlockEntry {
                                block_index: index,
                                phase: AssistantPhase::Final,
                                content,
                                collapsed: false,
                                streaming: true,
                            },
                        });
                        self.scrub_materialized_tool_markup();
                    }
                    StreamBlock::Thinking { content, collapsed } => {
                        let updated_existing =
                            if let Some(active) = self.task_doc.active_pulse.as_mut() {
                                if let Some(PulseEntry::AssistantBlock { block, .. }) =
                                    active.entries.last_mut()
                                {
                                    if block.block_index == index {
                                        block.phase = AssistantPhase::Thinking;
                                        block.collapsed = collapsed;
                                        block.streaming = true;
                                        if !content.is_empty() {
                                            block.content = content.clone();
                                        }
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                        if updated_existing {
                            self.clamp_transcript_after_mutation();
                            self.preserve_transcript_scroll_on_growth(previous_output_len);
                            return;
                        }
                        let step_id = self.alloc_step_id();
                        self.append_turn_entry(PulseEntry::AssistantBlock {
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
                        id,
                        name,
                        input,
                        status,
                    } => {
                        self.scrub_materialized_tool_markup();
                        let exists = if let Some(active) = self.task_doc.active_pulse.as_mut() {
                            active.entries.iter_mut().any(|e| {
                                if let PulseEntry::ToolCall {
                                    id: existing_id,
                                    status: existing_status,
                                    ..
                                } = e
                                    && *existing_id == id
                                {
                                    *existing_status = status.clone();
                                    return true;
                                }
                                false
                            })
                        } else {
                            false
                        };
                        if !exists {
                            let step_id = self.alloc_step_id();
                            self.append_turn_entry(PulseEntry::ToolCall {
                                step_id,
                                id,
                                name,
                                input,
                                status: crate::state::ToolStatus::Pending,
                            });
                            if self.timeline_follow_mode {
                                let total = self.timeline_entry_count();
                                self.selected_timeline_index = total.saturating_sub(1);
                                self.inspector_scroll_offset = 0;
                            }
                        }
                    }
                    StreamBlock::ToolResult {
                        tool_call_id,
                        output,
                        is_error,
                    } => {
                        if let Some(active) = self.task_doc.active_pulse.as_mut() {
                            for entry in &mut active.entries {
                                if let PulseEntry::ToolCall { id, status, .. } = entry
                                    && *id == tool_call_id
                                {
                                    *status = if is_error {
                                        crate::state::ToolStatus::Error
                                    } else {
                                        crate::state::ToolStatus::Complete
                                    };
                                    break;
                                }
                            }
                        }
                        if !is_error {
                            let name_and_input =
                                self.task_doc.active_pulse.as_ref().and_then(|a| {
                                    a.entries.iter().rev().find_map(|e| {
                                        if let PulseEntry::ToolCall {
                                            id, name, input, ..
                                        } = e
                                            && *id == tool_call_id
                                        {
                                            return Some((name.clone(), input.clone()));
                                        }
                                        None
                                    })
                                });
                            if let Some((name, input)) = name_and_input {
                                let mut changed = std::collections::BTreeSet::new();
                                note_changed_files_from_tool_call(&mut changed, &name, &input);
                                if !changed.is_empty() {
                                    self.invalidate_file_prompt_entries();
                                    if let Some(active) = self.task_doc.active_pulse.as_mut() {
                                        active.changed_files.extend(changed);
                                    }
                                }
                            }
                        }
                        let step_id = self.alloc_step_id();
                        self.append_turn_entry(PulseEntry::ToolResult {
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
                if let Some(active) = self.task_doc.active_pulse.as_mut() {
                    active.entries.iter_mut().rev().any(|e| {
                        if let PulseEntry::AssistantBlock { block, .. } = e
                            && block.block_index == index
                        {
                            block.content.push_str(&delta);
                            self.stream_uses_structured_final_output = true;
                            return true;
                        }
                        false
                    });
                }
                self.scrub_materialized_tool_markup();
                self.clamp_transcript_after_mutation();
            }

            UiUpdate::ToolCallArgumentsUpdated {
                tool_call_id,
                tool_name,
                arguments,
            } => {
                let previous_output_len = self.expanded_output_row_count();
                if let Some(active) = self.task_doc.active_pulse.as_mut() {
                    for entry in active.entries.iter_mut().rev() {
                        if let PulseEntry::ToolCall {
                            id, name, input, ..
                        } = entry
                            && *id == tool_call_id
                        {
                            if let Some(updated_name) = tool_name {
                                *name = updated_name;
                            }
                            *input = arguments;
                            break;
                        }
                    }
                }
                self.clamp_transcript_after_mutation();
                self.preserve_transcript_scroll_on_growth(previous_output_len);
            }

            UiUpdate::StreamBlockComplete { index } => {
                if let Some(active) = self.task_doc.active_pulse.as_mut() {
                    for entry in active.entries.iter_mut().rev() {
                        if let PulseEntry::AssistantBlock { block, .. } = entry
                            && block.block_index == index
                        {
                            block.streaming = false;
                            break;
                        }
                    }
                }
            }

            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name,
                input_preview,
                response_tx,
            }) => {
                if self
                    .task_doc
                    .active_pulse
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
                            .info
                            .active_grants
                            .get(&cap)
                            .copied()
                            .map(|scope| (cap, scope))
                    })
                {
                    if matches!(scope, ApprovalScope::Once) {
                        self.task_doc.info.active_grants.remove(&capability);
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
                if let Some(t) = self.task_doc.active_pulse.as_mut() {
                    t.command_sessions.clear();
                }
                self.last_error_message = None;
                if let Some(result) = last_validation_result
                    && let Some(edit_loop) = self.active_edit_loop.as_mut()
                {
                    edit_loop.set_last_validation_result(result);
                }
                self.resolve_pending_approval(false, ctx);
                self.resolve_pending_patch_approval(false);
                if let Some(active) = self.task_doc.active_pulse.as_mut() {
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
                            .map(|e| format!("[edit loop reached max pulses — last error: {e}]"))
                            .unwrap_or_else(|| "[edit loop reached max pulses]".to_string());
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

                if self.task_doc.active_pulse.is_some() {
                    let turn_outcome = match &outcome {
                        EditLoopOutcome::Success { .. } => PulseOutcome::Completed,
                        EditLoopOutcome::MaxTurnsReached { .. } => PulseOutcome::MaxTurnsReached,
                        EditLoopOutcome::ApprovalDenied => PulseOutcome::Cancelled,
                        EditLoopOutcome::Cancelled => PulseOutcome::Cancelled,
                    };
                    let turn_tokens = ctx.session_tokens_rollup().last_pulse();
                    self.task_doc_condenser.finish_turn(
                        &mut self.task_doc,
                        turn_outcome,
                        turn_tokens,
                        crate::runtime::session_task::now_millis(),
                    );
                    if matches!(outcome, EditLoopOutcome::Success { .. }) {
                        self.task_doc.info.status = TaskStatus::Completed;
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
                    .active_pulse
                    .as_mut()
                    .and_then(|t| t.command_sessions.get_mut(&session_id))
                {
                    session.pid = pid;
                }
            }

            UiUpdate::CommandSessionFinished { session_id } => {
                if let Some(active) = self.task_doc.active_pulse.as_mut() {
                    active.command_sessions.remove(&session_id);
                }
                self.complete_turn_if_idle(ctx);
            }

            UiUpdate::PulseComplete => {
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
                if let Some(t) = self.task_doc.active_pulse.as_mut() {
                    t.command_sessions.clear();
                }
                self.resolve_pending_approval(false, ctx);
                self.resolve_pending_patch_approval(false);
                self.last_turn_duration = self.turn_started_at.map(|s| s.elapsed());
                self.last_error_message = Some(msg.clone());
                self.push_document_notice(msg.clone(), NoticeSeverity::Error);
                if let Some(active) = self.task_doc.active_pulse.as_mut() {
                    active.cancel_pending = false;
                }
                self.read_only_turn_active = false;
                self.transcript_scroll_offset = 0;
                self.inspector_scroll_offset = 0;
                let tokens = ctx.session_tokens_rollup().last_pulse();
                self.task_doc_condenser.finish_turn(
                    &mut self.task_doc,
                    PulseOutcome::Failed { message: msg },
                    tokens,
                    crate::runtime::session_task::now_millis(),
                );
                self.set_task_status(TaskStatus::Failed);
                self.turn_completion_pending = false;
                self.reset_turn_capture();
            }
        }
    }
}
