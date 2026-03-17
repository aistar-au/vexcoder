use super::*;

impl TuiMode {
    pub(super) fn reset_conversation_window(&mut self, ctx: &RuntimeContext) {
        ctx.clear_conversation();
        self.history_state.lines.clear();
        self.history_state.turn_in_progress = false;
        self.history_state.cancel_pending = false;
        self.command_sessions.clear();
        self.history_state.active_assistant_index = None;
        self.history_state.scroll_offset = 0;
        self.history_state.auto_follow = true;
        self.active_stream_blocks.clear();
        self.last_assembled_context = None;
        self.read_only_turn_active = false;
        self.reset_turn_capture();
    }

    pub(super) fn apply_resumed_task(&mut self, state: TaskState, ctx: &RuntimeContext) {
        let restored_id = state.id.clone();
        let status = format!("{:?}", state.status);
        self.current_task = state;
        if let Some(path) = self.current_task.instructions_path.clone() {
            self.instructions_path = Some(path);
        } else {
            self.current_task.instructions_path = self.instructions_path.clone();
        }
        self.active_edit_loop = None;
        ctx.reset_session_tokens();
        self.reset_conversation_window(ctx);
        self.push_history_line(format!("[resumed: {restored_id} status={status}]"));
    }

    pub(super) fn reset_turn_capture(&mut self) {
        self.current_turn_input.clear();
        self.current_turn_response.clear();
        self.current_turn_changed_files.clear();
        self.current_turn_command_history.clear();
        self.current_turn_tool_invocations.clear();
        self.pending_turn_tool_calls.clear();
        self.selected_timeline_index = 0;
    }

    pub(super) fn begin_turn_capture(&mut self, input: String) {
        self.reset_turn_capture();
        self.current_turn_input = input;
        self.current_task.status = TaskStatus::Running;
    }

    pub(super) fn begin_command_session(&mut self, command: String) -> u64 {
        let session_id = self.next_command_session_id;
        self.begin_command_session_with_id(session_id, command);
        session_id
    }

    pub(super) fn begin_command_session_with_id(&mut self, session_id: u64, command: String) {
        self.next_command_session_id = self
            .next_command_session_id
            .max(session_id.saturating_add(1));
        if self
            .command_sessions
            .iter()
            .any(|session| session.id == session_id)
        {
            return;
        }
        self.command_sessions.push(CommandSessionState {
            id: session_id,
            command,
            pid: None,
            status: "running".to_string(),
        });
        self.current_task.status = TaskStatus::Running;
    }

    pub(super) fn commit_completed_turn(&mut self, ctx: &RuntimeContext) {
        if self.current_turn_input.trim().is_empty()
            && self.current_turn_response.trim().is_empty()
            && self.current_turn_changed_files.is_empty()
            && self.current_turn_command_history.is_empty()
            && self.current_turn_tool_invocations.is_empty()
        {
            self.current_task.status = TaskStatus::Completed;
            self.reset_turn_capture();
            return;
        }

        let changed_files = self
            .current_turn_changed_files
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        for path in &changed_files {
            let path_buf = PathBuf::from(path);
            if !self
                .current_task
                .changed_files
                .iter()
                .any(|existing| existing == &path_buf)
            {
                self.current_task.changed_files.push(path_buf);
            }
        }

        let command_history = std::mem::take(&mut self.current_turn_command_history);
        self.current_task
            .command_history
            .extend(command_history.iter().cloned());
        self.current_task.instructions_path = self.instructions_path.clone();
        self.current_task.status = TaskStatus::Completed;
        self.current_task.turns.push(TurnEvidenceState {
            input: std::mem::take(&mut self.current_turn_input),
            response: std::mem::take(&mut self.current_turn_response),
            changed_files,
            command_history,
            tool_invocations: std::mem::take(&mut self.current_turn_tool_invocations),
            tokens: ctx.session_tokens_snapshot().last_turn(),
        });

        let dir = TaskState::state_dir();
        if let Err(error) = self.current_task.save(&dir) {
            self.push_history_line(format!("[state] save failed: {error}"));
        }
        self.reset_turn_capture();
    }

    pub(super) fn summarize_usage_line_suffix(estimated: bool) -> &'static str {
        if estimated {
            " (estimated)"
        } else {
            ""
        }
    }
}
