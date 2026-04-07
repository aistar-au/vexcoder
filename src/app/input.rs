use super::*;

impl TuiMode {
    pub(super) fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext) {
        if self.overlay_active() {
            if self.overlay_state.pending_memory_clear {
                self.handle_memory_clear_input(&input);
                return;
            } else if self.overlay_state.pending_resume_selection.is_some() {
                self.handle_resume_selection_input(&input, ctx);
                return;
            } else if self.patch_overlay_active() {
                self.handle_patch_overlay_input(&input);
                return;
            } else {
                self.handle_approval_input(&input, ctx);
                return;
            }
        }

        if self.task_doc.active_turn.is_some() {
            let trimmed = input.trim();
            let reentrant_edit_command =
                self.active_edit_loop.is_some() && Self::is_reentrant_edit_command(trimmed);
            if reentrant_edit_command {
                self.push_history_line(format!("> {input}"));
                self.push_history_line(String::new());
                let _ = self.try_handle_slash_command(&input, ctx);
                return;
            }
            if self
                .task_doc
                .active_turn
                .as_ref()
                .is_some_and(|t| t.cancel_pending)
            {
                self.push_history_line(
                    "[busy - cancelling current turn, input ignored]".to_string(),
                );
            } else {
                let trimmed = input.trim();
                if let Some(command) = trimmed.strip_prefix('!') {
                    if self.command_session_active() && !command.trim().is_empty() {
                        self.push_history_line(format!("> {input}"));
                        self.push_history_line(String::new());
                        self.handle_bang_command(command, ctx);
                        return;
                    }
                }
                self.push_history_line("[busy - turn in progress, input ignored]".to_string());
            }
            return;
        }

        self.pending_quit = false;
        self.quit_requested = false;

        let turn_input = self.expand_inline_file_tokens(&input);

        let trimmed = turn_input.trim();
        if let Some(command) = trimmed.strip_prefix('!') {
            self.push_history_line(format!("> {input}"));
            self.push_history_line(String::new());
            self.handle_bang_command(command, ctx);
            return;
        }

        if turn_input.starts_with('/') {
            self.push_history_line(format!("> {input}"));
            self.push_history_line(String::new());
            if self.try_handle_slash_command(&turn_input, ctx) {
                return;
            }
        }

        // Begin the turn; transcript_projection will render the waiting
        // placeholder via TurnEntry::UserInput — no separate
        // push_history_line needed.
        self.begin_turn_capture(turn_input.clone());

        #[cfg(test)]
        {
            self.last_turn_input = Some(turn_input.clone());
        }

        ctx.start_turn(turn_input);
    }

    pub(super) fn on_interrupt(&mut self, ctx: &mut RuntimeContext) {
        if self.task_doc.active_turn.is_some() {
            if self
                .task_doc
                .active_turn
                .as_ref()
                .is_some_and(|t| t.cancel_pending)
            {
                return;
            }
            ctx.cancel_turn();
            self.resolve_pending_approval(false, ctx);
            self.resolve_pending_patch_approval(false);
            if let Some(active) = self.task_doc.active_turn.as_mut() {
                active.cancel_pending = true;
            }
            let has_command_sessions = self
                .task_doc
                .active_turn
                .as_ref()
                .is_some_and(|t| !t.command_sessions.is_empty());
            if has_command_sessions {
                if let Some(active) = self.task_doc.active_turn.as_mut() {
                    for session in active.command_sessions.values_mut() {
                        session.status = "cancelling".to_string();
                    }
                }
                self.set_task_status(TaskStatus::Cancelling);
                self.push_history_line("[command session cancellation requested]".to_string());
            } else {
                self.push_history_line("[turn cancellation requested]".to_string());
            }
            self.pending_quit = false;
            self.quit_requested = false;
            return;
        }

        if self.pending_quit {
            self.quit_requested = true;
        } else {
            self.pending_quit = true;
            self.push_history_line("[press Ctrl+C again to exit]".to_string());
        }
    }
}
