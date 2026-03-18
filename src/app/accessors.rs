use super::*;

impl TuiMode {
    fn human_mode_status(&self) -> &'static str {
        if self.overlay_active() {
            "Waiting for approval"
        } else if self.command_session_active() {
            "Command session running"
        } else if self.pending_quit {
            "Confirm quit"
        } else if self.history_state.cancel_pending {
            "Cancelling turn"
        } else if self.history_state.turn_in_progress {
            "Running"
        } else {
            "Ready"
        }
    }

    fn human_approval_status(&self) -> &'static str {
        if self.overlay_active() {
            "approval pending"
        } else if self.overlay_state.auto_approve_session {
            "auto-approve on"
        } else {
            "approvals off"
        }
    }

    pub fn status_line(&self) -> String {
        let history_rows =
            history_visual_line_count(&self.history_state.lines, self.history_content_width.get());
        format!(
            "{} · {} · history {} · repo {} · instructions {}",
            self.human_mode_status(),
            self.human_approval_status(),
            history_rows,
            self.repo_label,
            self.instructions_path.as_deref().unwrap_or("none")
        )
    }

    pub fn current_task_id(&self) -> String {
        self.current_task.id.clone()
    }

    pub fn overlay_active(&self) -> bool {
        self.overlay_state.pending_approval.is_some()
            || self.overlay_state.pending_patch_approval.is_some()
            || self.overlay_state.pending_resume_selection.is_some()
            || self.overlay_state.pending_memory_clear
    }

    pub(super) fn patch_overlay_active(&self) -> bool {
        self.overlay_state.pending_patch_approval.is_some()
    }

    pub fn history_lines(&self) -> &[String] {
        &self.history_state.lines
    }

    pub fn active_assistant_index(&self) -> Option<usize> {
        self.history_state.active_assistant_index
    }

    pub fn history_scroll_offset(&self) -> usize {
        self.history_state.scroll_offset
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    pub fn pending_patch_overlay(&self) -> Option<(&str, usize)> {
        self.overlay_state
            .pending_patch_approval
            .as_ref()
            .map(|pending| (pending.patch_preview.as_str(), pending.scroll_offset))
    }

    pub fn pending_tool_overlay(&self) -> Option<(&str, &str, bool)> {
        self.overlay_state.pending_approval.as_ref().map(|pending| {
            (
                pending.tool_name.as_str(),
                pending.input_preview.as_str(),
                self.overlay_state.auto_approve_session,
            )
        })
    }

    pub fn pending_memory_clear_overlay(&self) -> bool {
        self.overlay_state.pending_memory_clear
    }

    pub fn command_session_active(&self) -> bool {
        !self.command_sessions.is_empty()
    }

    pub fn set_history_content_width(&self, width: usize) {
        self.history_content_width.set(width.max(1));
    }

    /// Total number of timeline entries available for selection.
    /// Mirrors the entry count produced by `task_timeline_entries()`.
    pub(super) fn timeline_entry_count(&self) -> usize {
        if !self.command_sessions.is_empty() {
            return self.command_sessions.len().max(1);
        }

        // Use current turn data if available, otherwise fall back to last turn.
        let (input_text, tool_count) = if self.history_state.turn_in_progress
            || !self.current_turn_input.trim().is_empty()
            || !self.current_turn_tool_invocations.is_empty()
            || !self.pending_turn_tool_calls.is_empty()
        {
            (
                &self.current_turn_input,
                self.current_turn_tool_invocations.len() + self.pending_turn_tool_calls.len(),
            )
        } else {
            (
                &self.last_turn_input_display,
                self.last_turn_tool_invocations.len(),
            )
        };

        let mut count = 0;
        if !input_text.trim().is_empty() {
            count += 1;
        }
        count += tool_count;
        count.max(1)
    }
}
