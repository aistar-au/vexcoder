use super::*;

impl TuiMode {
    pub(super) fn on_model_update(&mut self, update: UiUpdate, ctx: &mut RuntimeContext) {
        match update {
            UiUpdate::TranscriptLine(line) => {
                let previous_output_len = self.task_output_view().1.len();
                if self.history_state.turn_in_progress {
                    if !self.current_turn_response.is_empty() {
                        self.current_turn_response.push('\n');
                    }
                    self.current_turn_response.push_str(&line);
                }
                self.push_history_line(line);
                self.preserve_transcript_scroll_on_growth(previous_output_len);
            }
            UiUpdate::StreamDelta(text) => {
                let previous_output_len = self.task_output_view().1.len();
                if self.history_state.cancel_pending {
                    return;
                }
                self.current_turn_response.push_str(&text);
                let idx = match self.history_state.active_assistant_index {
                    Some(idx) => idx,
                    None => {
                        if !self.history_state.turn_in_progress {
                            return;
                        }
                        self.push_history_line(String::new());
                        let idx = self.history_state.lines.len() - 1;
                        self.history_state.active_assistant_index = Some(idx);
                        idx
                    }
                };
                if let Some(line) = self.history_state.lines.get_mut(idx) {
                    line.push_str(&text);
                    *line = sanitize_assistant_text(line);
                }
                if self.history_state.auto_follow {
                    self.set_scroll_to_bottom();
                }
                self.preserve_transcript_scroll_on_growth(previous_output_len);
            }
            UiUpdate::StreamBlockStart { index, block } => {
                match &block {
                    StreamBlock::ToolCall {
                        id, name, input, ..
                    } => {
                        let step_id = self.next_step_id;
                        self.next_step_id += 1;
                        self.pending_turn_tool_calls.insert(
                            id.clone(),
                            PendingTurnToolCall {
                                step_id,
                                name: name.clone(),
                                input: input.clone(),
                            },
                        );
                        // Auto-advance timeline selection when follow mode is on.
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
                        if let Some(pending) = self.pending_turn_tool_calls.remove(tool_call_id) {
                            if !*is_error {
                                note_changed_files_from_tool_call(
                                    &mut self.current_turn_changed_files,
                                    &pending.name,
                                    &pending.input,
                                );
                            }
                            if let Some(evidence) =
                                command_evidence_from_tool_result(&pending.name, *is_error)
                            {
                                self.current_turn_command_history.push(evidence);
                            }
                            self.current_turn_tool_invocations
                                .push(ToolInvocationSummary {
                                    step_id: pending.step_id,
                                    name: pending.name,
                                    outcome: summarize_tool_outcome(output, *is_error).to_string(),
                                });
                        }
                    }
                    StreamBlock::Thinking { .. } | StreamBlock::FinalText { .. } => {}
                }
                self.active_stream_blocks.insert(index, block);
            }
            UiUpdate::StreamBlockDelta { index, delta } => {
                if let Some(block) = self.active_stream_blocks.get_mut(&index) {
                    match block {
                        StreamBlock::Thinking { content, .. } => content.push_str(&delta),
                        StreamBlock::FinalText { content } => content.push_str(&delta),
                        StreamBlock::ToolCall { .. } | StreamBlock::ToolResult { .. } => {}
                    }
                }
            }
            UiUpdate::StreamBlockComplete { index } => {
                self.active_stream_blocks.remove(&index);
            }
            UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
                tool_name,
                input_preview,
                response_tx,
            }) => {
                if self.history_state.cancel_pending {
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
                    let _ = response_tx.send(true);
                    self.push_history_line(format!("[auto-approved tool: {tool_name} session]"));
                    return;
                }

                if let Some((capability, scope)) =
                    capability_for_tool_name(&tool_name).and_then(|capability| {
                        self.current_task
                            .active_grants
                            .get(&capability)
                            .copied()
                            .map(|scope| (capability, scope))
                    })
                {
                    if matches!(scope, ApprovalScope::Once) {
                        self.current_task.active_grants.remove(&capability);
                    }
                    let _ = response_tx.send(true);
                    self.push_history_line(format!(
                        "[auto-approved tool: {tool_name} {} grant]",
                        scope_to_label(scope)
                    ));
                    return;
                }

                let summary = summarize_tool_approval_context(&tool_name, &input_preview);
                self.push_history_line(format!("[tool approval requested: {summary}]"));
                self.overlay_state.pending_approval = Some(PendingApproval {
                    tool_name,
                    input_preview,
                    action: PendingApprovalAction::Tool(response_tx),
                });
            }
            UiUpdate::EditLoopComplete {
                outcome,
                last_validation_result,
            } => {
                self.command_sessions.clear();
                self.last_error_message = None;
                if let Some(result) = last_validation_result {
                    if let Some(edit_loop) = self.active_edit_loop.as_mut() {
                        edit_loop.set_last_validation_result(result);
                    }
                }
                self.resolve_pending_approval(false, ctx);
                self.resolve_pending_patch_approval(false);
                self.active_stream_blocks.clear();
                self.history_state.cancel_pending = false;
                self.history_state.turn_in_progress = false;
                self.history_state.active_assistant_index = None;
                match outcome {
                    EditLoopOutcome::Success {
                        patch_applied,
                        validate_passed,
                    } => {
                        let summary = format!(
                            "[edit loop complete: patch_applied={} validate_passed={}]",
                            patch_applied, validate_passed
                        );
                        self.push_history_line(summary);
                    }
                    EditLoopOutcome::MaxTurnsReached { last_error } => {
                        let summary = match last_error {
                            Some(err) => {
                                format!("[edit loop reached max turns — last error: {err}]")
                            }
                            None => "[edit loop reached max turns]".to_string(),
                        };
                        self.push_history_line(summary);
                    }
                    EditLoopOutcome::ApprovalDenied => {
                        self.push_history_line("[edit loop aborted: approval denied]".to_string());
                    }
                    EditLoopOutcome::Cancelled => {
                        self.push_history_line("[edit loop cancelled]".to_string());
                    }
                }
                if self.history_state.auto_follow {
                    self.set_scroll_to_bottom();
                } else {
                    self.clamp_scroll_offset();
                }
                self.transcript_scroll_offset = 0;
                self.inspector_scroll_offset = 0;
            }
            UiUpdate::CommandSessionStarted {
                session_id,
                command,
            } => {
                self.begin_command_session_with_id(session_id, command);
            }
            UiUpdate::CommandSessionAttached { session_id, pid } => {
                if let Some(session) = self
                    .command_sessions
                    .iter_mut()
                    .find(|session| session.id == session_id)
                {
                    session.pid = pid;
                }
            }
            UiUpdate::CommandSessionFinished { session_id } => {
                if let Some(pos) = self
                    .command_sessions
                    .iter()
                    .position(|session| session.id == session_id)
                {
                    self.command_sessions.remove(pos);
                }
            }
            UiUpdate::TurnComplete => {
                if !self.command_sessions.is_empty() {
                    return;
                }
                self.last_error_message = None;
                self.resolve_pending_approval(false, ctx);
                self.resolve_pending_patch_approval(false);
                self.active_stream_blocks.clear();
                self.commit_completed_turn(ctx);
                self.history_state.cancel_pending = false;
                self.history_state.turn_in_progress = false;
                self.history_state.active_assistant_index = None;
                self.read_only_turn_active = false;
                if self.history_state.auto_follow {
                    self.set_scroll_to_bottom();
                } else {
                    self.clamp_scroll_offset();
                }
                self.transcript_scroll_offset = 0;
                self.inspector_scroll_offset = 0;
            }
            UiUpdate::Error(msg) => {
                self.command_sessions.clear();
                self.resolve_pending_approval(false, ctx);
                self.resolve_pending_patch_approval(false);
                self.active_stream_blocks.clear();
                self.last_turn_duration = self.turn_started_at.map(|started| started.elapsed());
                self.last_error_message = Some(msg.clone());
                self.reset_turn_capture();
                self.history_state.cancel_pending = false;
                self.push_history_line(format!("[error] {msg}"));
                self.current_task.status = TaskStatus::Failed;
                self.history_state.turn_in_progress = false;
                self.history_state.active_assistant_index = None;
                self.read_only_turn_active = false;
                self.transcript_scroll_offset = 0;
                self.inspector_scroll_offset = 0;
            }
        }
    }
}
