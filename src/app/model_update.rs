use super::*;

impl TuiMode {
    pub(super) fn on_model_update(&mut self, update: UiUpdate, ctx: &mut RuntimeContext) {
        match update {
            UiUpdate::TranscriptLine(line) => {
                let previous_output_len = self.task_output_view().1.len();
                if self.history_state.turn_in_progress {
                    self.current_task.status = TaskStatus::Running;
                }
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
                if self.history_state.turn_in_progress {
                    self.current_task.status = TaskStatus::Running;
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
                    // Clear the waiting placeholder on first real content.
                    if line.starts_with("[waiting for response...]") {
                        line.clear();
                    }
                    line.push_str(&text);
                    *line = sanitize_assistant_text(line);
                }
                if self.history_state.auto_follow {
                    self.set_scroll_to_bottom();
                }
                self.preserve_transcript_scroll_on_growth(previous_output_len);
            }
            UiUpdate::StreamBlockStart { index, block } => {
                if self.history_state.turn_in_progress {
                    self.current_task.status = TaskStatus::Running;
                }
                // Clear the waiting placeholder when a tool block arrives.
                if let Some(idx) = self.history_state.active_assistant_index {
                    if let Some(line) = self.history_state.lines.get_mut(idx) {
                        if line.starts_with("[waiting for response...]") {
                            line.clear();
                        }
                    }
                }
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
                                input_preview: preview_tool_input(
                                    name,
                                    input,
                                    ToolPreviewStyle::Compact,
                                    crate::edit_diff::DEFAULT_EDIT_DIFF_CONTEXT_LINES,
                                ),
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
                            self.overlay_state
                                .approved_tool_steps
                                .remove(&pending.step_id);
                            if !*is_error {
                                note_changed_files_from_tool_call(
                                    &mut self.current_turn_changed_files,
                                    &pending.name,
                                    &pending.input,
                                );
                                self.invalidate_file_prompt_entries();
                            }
                            if let Some(evidence) =
                                command_evidence_from_tool_result(&pending.name, *is_error)
                            {
                                self.current_turn_command_history.push(evidence);
                            }
                            // Immediately push a verb-first line so the user
                            // sees tool progress without waiting for the model
                            // to produce response text.
                            let previous_output_len = self.task_output_view().1.len();
                            let para = verb_first_tool_paragraph(
                                &pending.name,
                                &pending.input,
                                output,
                                *is_error,
                            );
                            self.push_history_line(para);
                            self.preserve_transcript_scroll_on_growth(previous_output_len);

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
                if self.history_state.turn_in_progress {
                    self.current_task.status = TaskStatus::Running;
                }
                if let Some(block) = self.active_stream_blocks.get_mut(&index) {
                    match block {
                        StreamBlock::Thinking { content, .. } => content.push_str(&delta),
                        StreamBlock::FinalText { content } => content.push_str(&delta),
                        StreamBlock::ToolCall { .. } | StreamBlock::ToolResult { .. } => {}
                    }
                }
            }
            UiUpdate::StreamBlockComplete { index } => {
                if self.history_state.turn_in_progress {
                    self.current_task.status = TaskStatus::Running;
                }
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
                    let step_id = self.pending_tool_step_id(&tool_name, &input_preview);
                    self.mark_tool_step_approved(step_id);
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
                    let step_id = self.pending_tool_step_id(&tool_name, &input_preview);
                    self.mark_tool_step_approved(step_id);
                    let _ = response_tx.send(true);
                    self.push_history_line(format!(
                        "[auto-approved tool: {tool_name} {} grant]",
                        scope_to_label(scope)
                    ));
                    return;
                }

                let summary = summarize_tool_approval_context(&tool_name, &input_preview);
                let step_id = self.pending_tool_step_id(&tool_name, &input_preview);
                self.push_history_line(format!("[tool approval requested: {summary}]"));
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
                        self.set_task_status(TaskStatus::Completed);
                        let summary = format!(
                            "[edit loop complete: patch_applied={} validate_passed={}]",
                            patch_applied, validate_passed
                        );
                        self.push_history_line(summary);
                    }
                    EditLoopOutcome::MaxTurnsReached { last_error } => {
                        self.set_task_status(TaskStatus::MaxTurnsReached);
                        let summary = match last_error {
                            Some(err) => {
                                format!("[edit loop reached max turns — last error: {err}]")
                            }
                            None => "[edit loop reached max turns]".to_string(),
                        };
                        self.push_history_line(summary);
                    }
                    EditLoopOutcome::ApprovalDenied => {
                        self.set_task_status(TaskStatus::Cancelled);
                        self.push_history_line("[edit loop aborted: approval denied]".to_string());
                    }
                    EditLoopOutcome::Cancelled => {
                        self.set_task_status(TaskStatus::Cancelled);
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
                if self.command_sessions.is_empty() {
                    self.complete_turn_if_idle(ctx);
                }
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
                let turn_index = self.current_task.turns.len();
                self.current_task
                    .context_compaction
                    .push(ContextCompactionRecord {
                        turn_index,
                        messages_before,
                        messages_after,
                        summary,
                    });
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
                self.persist_current_task_state();
                self.history_state.turn_in_progress = false;
                self.history_state.active_assistant_index = None;
                self.read_only_turn_active = false;
                self.transcript_scroll_offset = 0;
                self.inspector_scroll_offset = 0;
            }
        }
    }
}

/// Derive a verb-first one-liner from a completed tool call so the transcript
/// shows immediate progress instead of a blank screen while the model thinks.
fn verb_first_tool_paragraph(
    name: &str,
    input: &serde_json::Value,
    output: &str,
    is_error: bool,
) -> String {
    if is_error {
        let short = output
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("error");
        let capped = &short[..short.floor_char_boundary(60)];
        return format!("[!] {name}: {capped}");
    }

    let str_arg = |keys: &[&str]| -> &str {
        keys.iter()
            .find_map(|k| {
                input
                    .get(*k)
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.is_empty())
            })
            .unwrap_or("")
    };

    match name {
        "search_files" | "search_content" | "search" | "find_files" | "codebase_search" => {
            let pat = str_arg(&["pattern", "query", "q", "text", "needle"]);
            let path = str_arg(&["path", "directory", "dir"]);
            if !pat.is_empty() && !path.is_empty() {
                format!("Searched {pat:?} in {path}")
            } else if !pat.is_empty() {
                format!("Searched {pat:?}")
            } else {
                format!("{name}: ok")
            }
        }
        "read_file" => {
            let path = str_arg(&["path", "file"]);
            let lines = output.lines().count();
            if !path.is_empty() {
                format!("Read {path} ({lines} lines)")
            } else {
                "Read: (no path given)".to_string()
            }
        }
        "list_files" | "list_directory" => {
            let path = str_arg(&["path", "dir", "directory", "root"]);
            let count = output.lines().count();
            if !path.is_empty() {
                format!("Listed {path} ({count} entries)")
            } else {
                format!("Listed workspace ({count} entries)")
            }
        }
        "write_file" => {
            let path = str_arg(&["path", "file"]);
            let lines = str_arg(&["content"]).lines().count();
            if !path.is_empty() && lines > 0 {
                format!("Wrote {lines} lines to {path}")
            } else if !path.is_empty() {
                format!("Wrote {path}")
            } else {
                format!("{name}: ok")
            }
        }
        "edit_file" => {
            let path = str_arg(&["path", "file"]);
            if !path.is_empty() {
                format!("Edited {path}")
            } else {
                format!("{name}: ok")
            }
        }
        "git_status" => "Checked git status".to_string(),
        "git_diff" => "Fetched git diff".to_string(),
        "git_log" => "Fetched git log".to_string(),
        "git_show" => "Showed git object".to_string(),
        "git_add" => "Staged files".to_string(),
        "git_commit" => "Committed changes".to_string(),
        "run_command" => {
            let cmd = str_arg(&["command", "cmd"]);
            if !cmd.is_empty() {
                let capped = &cmd[..cmd.floor_char_boundary(60)];
                format!("Ran: {capped}")
            } else {
                format!("{name}: ok")
            }
        }
        "apply_patch" => "Applied patch".to_string(),
        _ => {
            let first = output
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or("");
            if first.is_empty() {
                format!("{name}: ok")
            } else {
                let capped = &first[..first.floor_char_boundary(60)];
                format!("{name}: {capped}")
            }
        }
    }
}
