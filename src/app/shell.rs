use super::*;

impl TuiMode {
    pub(super) fn handle_bang_command(&mut self, command: &str, ctx: &RuntimeContext) {
        let command = command.trim();
        if command.is_empty() {
            self.push_history_line("[shell] usage: !<command>".to_string());
            return;
        }

        if self.overlay_state.auto_approve_session {
            self.push_history_line("[auto-approved tool: run_command session]".to_string());
            self.start_command_session(command.to_string(), ctx);
            return;
        }

        if let Some(scope) = self
            .task_doc
            .meta
            .active_grants
            .get(&Capability::RunCommand)
            .copied()
        {
            if matches!(scope, ApprovalScope::Once) {
                self.task_doc
                    .meta
                    .active_grants
                    .remove(&Capability::RunCommand);
            }
            self.push_history_line(format!(
                "[auto-approved tool: run_command {} grant]",
                scope_to_label(scope)
            ));
            self.start_command_session(command.to_string(), ctx);
            return;
        }

        let summary = summarize_tool_approval_context("run_command", command);
        self.push_history_line(format!("[tool approval requested: {summary}]"));
        self.overlay_state.pending_approval = Some(PendingApproval {
            step_id: None,
            tool_name: "run_command".to_string(),
            input_preview: command.to_string(),
            action: PendingApprovalAction::InlineCommand(PendingInlineCommand {
                command: command.to_string(),
            }),
        });
        self.set_task_status(TaskStatus::AwaitingApproval);
    }

    pub(super) fn start_command_session(&mut self, command: String, ctx: &RuntimeContext) {
        let starting_batch = self
            .task_doc
            .active_turn
            .as_ref()
            .is_none_or(|t| t.command_sessions.is_empty());
        if starting_batch {
            self.begin_turn_capture(format!("!{command}"));
        }
        let session_id = self.begin_command_session(command.clone());

        let ctx = ctx.clone();
        let cancel = ctx.turn_cancellation_token();
        let working_dir = self.working_dir.clone();
        let sandbox = self.sandbox.clone();
        tokio::spawn(async move {
            let runner = DefaultCommandRunner::new();
            let request = match sandbox.wrap(shell_command_request(command.clone(), working_dir)) {
                Ok(request) => request,
                Err(error) => {
                    ctx.emit_transcript_line(format!("[command session] error: {error}"));
                    ctx.emit_command_session_finished(session_id);
                    ctx.emit_turn_complete();
                    return;
                }
            };
            let (output_tx, mut output_rx) = mpsc::channel(128);
            let mut handle = match runner.run_streaming(request, output_tx).await {
                Ok(handle) => handle,
                Err(error) => {
                    ctx.emit_transcript_line(format!("[command session] error: {error}"));
                    ctx.emit_command_session_finished(session_id);
                    ctx.emit_turn_complete();
                    return;
                }
            };
            ctx.emit_command_session_attached(session_id, handle.pid());
            ctx.emit_transcript_line(format_command_session_started(&command, handle.pid()));

            let mut cancel_requested = false;
            loop {
                tokio::select! {
                    _ = cancel.cancelled(), if !cancel_requested => {
                        cancel_requested = true;
                        let _ = handle.cancel();
                        ctx.emit_transcript_line("[command session cancellation requested]".to_string());
                    }
                    chunk = output_rx.recv() => {
                        match chunk {
                            Some(chunk) => {
                                for line in format_command_session_output(chunk) {
                                    ctx.emit_transcript_line(line);
                                }
                            }
                            None => break,
                        }
                    }
                }
            }

            match handle.wait().await {
                Ok(result) => {
                    if cancel_requested {
                        ctx.emit_transcript_line(format_command_session_cancelled());
                    } else {
                        ctx.emit_transcript_line(format_command_session_exit(result.exit_code));
                    }
                }
                Err(error) => {
                    ctx.emit_transcript_line(format!("[command session] error: {error}"));
                }
            }
            ctx.emit_command_session_finished(session_id);
            ctx.emit_turn_complete();
        });
    }
}
