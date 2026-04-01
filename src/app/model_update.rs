use super::*;
use crate::status_contract::is_waiting_placeholder;

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
                // Capture client-side TTFT on the very first content delta.
                if self.ttft.is_none() {
                    if let Some(started) = self.turn_started_at {
                        self.ttft = Some(started.elapsed());
                    }
                }
                self.current_turn_response.push_str(&text);
                let idx = match self.history_state.active_assistant_index {
                    Some(idx) => idx,
                    None => {
                        if !self.history_state.turn_in_progress {
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "[vex:debug] stale StreamDelta dropped ({} bytes)",
                                text.len()
                            );
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
                    if is_waiting_placeholder(line) {
                        line.clear();
                    }
                    line.push_str(&text);
                    *line = sanitize_assistant_text(line);
                }
                self.apply_auto_follow_or_clamp();
                self.preserve_transcript_scroll_on_growth(previous_output_len);
            }
            UiUpdate::ServerMetadata(metadata) => {
                let metadata = *metadata;
                if self.history_state.cancel_pending {
                    return;
                }
                if self.history_state.turn_in_progress {
                    self.current_task.status = TaskStatus::Running;
                }
                if let Some(progress) = metadata.prompt_progress {
                    self.current_turn_prompt_progress = Some(progress);
                }
                if let Some(timings) = metadata.timings {
                    self.current_turn_timings = Some(timings);
                }
            }
            UiUpdate::StreamBlockStart { index, block } => {
                if self.history_state.turn_in_progress {
                    self.current_task.status = TaskStatus::Running;
                }
                // Clear the waiting placeholder when a tool block arrives.
                if let Some(idx) = self.history_state.active_assistant_index {
                    if let Some(line) = self.history_state.lines.get_mut(idx) {
                        if is_waiting_placeholder(line) {
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
                        let pending = PendingTurnToolCall {
                            step_id,
                            name: name.clone(),
                            input_preview: preview_tool_input(
                                name,
                                input,
                                ToolPreviewStyle::Compact,
                                crate::edit_diff::DEFAULT_EDIT_DIFF_CONTEXT_LINES,
                            ),
                            input: input.clone(),
                        };
                        let previous_output_len = self.task_output_view().1.len();
                        let transcript_rows = super::layout::pending_tool_paragraph_rows(
                            &pending,
                            StepLifecycle::Running,
                        );
                        // Fold pending tool calls with the same name into
                        // the existing paragraph — only show detail rows
                        // (skip the [tool] header) to avoid per-file spam.
                        let is_pending_same_name = self
                            .last_pending_tool_name
                            .as_ref()
                            .map(|prev| prev == name)
                            .unwrap_or(false);
                        self.pending_turn_tool_calls.insert(id.clone(), pending);
                        self.tool_input_raw_buffers.insert(index, String::new());
                        if is_pending_same_name {
                            // Suppress the [tool] header, only emit detail lines.
                            for row in transcript_rows.iter().skip(1) {
                                self.push_history_line(row.clone());
                            }
                        } else {
                            for row in transcript_rows {
                                self.push_history_line(row);
                            }
                            // Track the name for subsequent pending calls.
                            self.last_pending_tool_name = Some(name.clone());
                        }
                        self.preserve_transcript_scroll_on_growth(previous_output_len);
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
                            let previous_output_len = self.task_output_view().1.len();
                            let transcript_rows = super::layout::completed_tool_paragraph_rows(
                                &pending.name,
                                &pending.input,
                                output,
                                *is_error,
                            );
                            // Fold consecutive identical tool calls into a
                            // single count-annotated line to prevent screen
                            // flooding when the model retries the same call.
                            // Also fold consecutive same-name tool calls
                            // (e.g. list_files for different paths) into a
                            // single paragraph with a count annotation.
                            let header = transcript_rows.first().cloned().unwrap_or_default();
                            let is_exact_duplicate = self
                                .last_completed_tool_header
                                .as_ref()
                                .map(|prev| *prev == header)
                                .unwrap_or(false);
                            let is_same_name = self
                                .last_completed_tool_name
                                .as_ref()
                                .map(|prev| *prev == pending.name)
                                .unwrap_or(false);

                            if is_exact_duplicate {
                                self.duplicate_tool_count += 1;
                                self.same_name_tool_count += 1;
                                // Replace the previous folded-count line if present.
                                if let Some(last_line) = self.history_state.lines.last_mut() {
                                    if last_line.starts_with("[detail] (repeated") {
                                        *last_line = format!(
                                            "[detail] (repeated {}\u{d7}, same call)",
                                            self.duplicate_tool_count
                                        );
                                    } else {
                                        self.push_history_line(format!(
                                            "[detail] (repeated {}\u{d7}, same call)",
                                            self.duplicate_tool_count
                                        ));
                                    }
                                }
                            } else if is_same_name {
                                // Same tool name but different target/args —
                                // fold into a paragraph: suppress the header
                                // and only append the detail/evidence rows.
                                self.duplicate_tool_count = 1;
                                self.same_name_tool_count += 1;
                                self.last_completed_tool_header = Some(header);
                                // Replace or append the batch count line.
                                if let Some(last_line) = self.history_state.lines.last_mut() {
                                    if last_line.starts_with("[detail] (")
                                        && last_line.contains("files)")
                                    {
                                        *last_line = format!(
                                            "[detail] ({} files)",
                                            self.same_name_tool_count
                                        );
                                    } else {
                                        // Append only the detail rows (skip the
                                        // [tool] header to avoid per-file repetition).
                                        for row in transcript_rows.iter().skip(1) {
                                            self.push_history_line(row.clone());
                                        }
                                        self.push_history_line(format!(
                                            "[detail] ({} files)",
                                            self.same_name_tool_count
                                        ));
                                    }
                                }
                            } else {
                                self.duplicate_tool_count = 1;
                                self.same_name_tool_count = 1;
                                self.last_completed_tool_header = Some(header);
                                self.last_completed_tool_name = Some(pending.name.clone());
                                for row in transcript_rows {
                                    self.push_history_line(row);
                                }
                            }
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
                        StreamBlock::ToolCall { id, .. } => {
                            let tool_id = id.clone();
                            if let Some(buf) = self.tool_input_raw_buffers.get_mut(&index) {
                                buf.push_str(&delta);
                                if let Some(pending) =
                                    self.pending_turn_tool_calls.get_mut(&tool_id)
                                {
                                    if let Ok(parsed) =
                                        serde_json::from_str::<serde_json::Value>(buf)
                                    {
                                        pending.input = parsed.clone();
                                        pending.input_preview =
                                            crate::tool_preview::preview_tool_input(
                                                &pending.name.clone(),
                                                &parsed,
                                                crate::tool_preview::ToolPreviewStyle::Compact,
                                                crate::edit_diff::DEFAULT_EDIT_DIFF_CONTEXT_LINES,
                                            );
                                    } else {
                                        pending.input_preview =
                                            crate::tool_preview::preview_partial_tool_input(
                                                &pending.name,
                                                buf,
                                            );
                                    }
                                }
                            }
                        }
                        StreamBlock::ToolResult { .. } => {}
                    }
                }
            }
            UiUpdate::StreamBlockComplete { index } => {
                if self.history_state.turn_in_progress {
                    self.current_task.status = TaskStatus::Running;
                }
                self.tool_input_raw_buffers.remove(&index);
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
                    self.push_history_line(format!(
                        "[approval] {tool_name} auto-approved for session"
                    ));
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
                        "[approval] {tool_name} auto-approved via {} grant",
                        scope_to_label(scope)
                    ));
                    return;
                }

                let summary = summarize_tool_approval_context(&tool_name, &input_preview);
                let step_id = self.pending_tool_step_id(&tool_name, &input_preview);
                let previous_output_len = self.task_output_view().1.len();
                for row in super::layout::tool_approval_paragraph_rows(
                    &format!("{summary} · awaiting approval"),
                    &input_preview,
                ) {
                    self.push_history_line(row);
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
                self.tool_input_raw_buffers.clear();
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
                self.apply_auto_follow_or_clamp();
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
                self.tool_input_raw_buffers.clear();
                self.last_turn_duration = self.turn_started_at.map(|started| started.elapsed());
                self.last_turn_timings = self.current_turn_timings.clone();
                self.last_error_message = Some(msg.clone());
                self.reset_turn_capture();
                self.history_state.cancel_pending = false;
                self.push_history_line(format!("[error] {msg}"));
                self.set_task_status(TaskStatus::Failed);
                self.history_state.turn_in_progress = false;
                self.history_state.active_assistant_index = None;
                self.read_only_turn_active = false;
                self.transcript_scroll_offset = 0;
                self.inspector_scroll_offset = 0;
            }
        }
    }
}
