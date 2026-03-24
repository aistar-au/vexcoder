use super::*;

impl TuiMode {
    pub(super) fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext) {
        if self.overlay_active() {
            if self.overlay_state.pending_memory_clear {
                self.handle_memory_clear_input(&input);
                return;
            } else if self.patch_overlay_active() {
                self.handle_patch_overlay_input(&input);
                return;
            } else {
                self.handle_approval_input(&input, ctx);
                return;
            }
        }

        if self.history_state.turn_in_progress {
            let trimmed = input.trim();
            let reentrant_edit_command =
                self.active_edit_loop.is_some() && Self::is_reentrant_edit_command(trimmed);
            if reentrant_edit_command {
                self.push_history_line(format!("> {input}"));
                self.push_history_line(String::new());
                let _ = self.try_handle_slash_command(&input, ctx);
                return;
            }
            if self.history_state.cancel_pending {
                self.push_history_line(
                    "[busy - cancelling current turn, input ignored]".to_string(),
                );
            } else {
                // Allow additional shell commands only while an existing
                // command-session batch is active. This avoids clobbering
                // model-turn capture state with unrelated inline commands.
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
        self.history_state.cancel_pending = false;
        self.push_history_line(format!("> {input}"));
        self.push_history_line(String::new());

        let turn_input = self.expand_inline_file_tokens(&input);

        let trimmed = turn_input.trim();
        if let Some(command) = trimmed.strip_prefix('!') {
            self.handle_bang_command(command, ctx);
            return;
        }

        if turn_input.starts_with('/') && self.try_handle_slash_command(&turn_input, ctx) {
            return;
        }

        // Show a waiting indicator until the first streaming token arrives.
        let wait_idx = self.history_state.lines.len() - 1;
        if let Some(line) = self.history_state.lines.get_mut(wait_idx) {
            *line = "[waiting for response...]".to_string();
        }
        self.history_state.active_assistant_index = Some(wait_idx);
        self.history_state.turn_in_progress = true;
        self.begin_turn_capture(turn_input.clone());

        #[cfg(test)]
        {
            self.last_turn_input = Some(turn_input.clone());
        }

        ctx.start_turn(turn_input);
    }

    pub(super) fn on_interrupt(&mut self, ctx: &mut RuntimeContext) {
        if self.history_state.turn_in_progress {
            if self.history_state.cancel_pending {
                return;
            }
            ctx.cancel_turn();
            self.resolve_pending_approval(false, ctx);
            self.resolve_pending_patch_approval(false);
            self.history_state.cancel_pending = true;
            if !self.command_sessions.is_empty() {
                for session in &mut self.command_sessions {
                    session.status = "cancelling".to_string();
                }
                self.current_task.status = TaskStatus::Cancelling;
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
