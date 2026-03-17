use super::*;

impl TuiMode {
    pub(super) fn mode_status_label(&self) -> &'static str {
        if self.overlay_active() {
            "overlay"
        } else if self.command_session_active() {
            "command-session"
        } else if self.pending_quit {
            "quit-arm"
        } else if self.history_state.cancel_pending {
            "cancelling"
        } else if self.history_state.turn_in_progress {
            "streaming"
        } else {
            "ready"
        }
    }

    pub(super) fn approval_status_label(&self) -> &'static str {
        if self.overlay_active() {
            "pending"
        } else if self.overlay_state.auto_approve_session {
            "auto"
        } else {
            "none"
        }
    }

    pub fn status_line(&self) -> String {
        let history_rows =
            history_visual_line_count(&self.history_state.lines, self.history_content_width.get());
        format!(
            "mode:{} approval:{} history:{} repo:{} inst:{}",
            self.mode_status_label(),
            self.approval_status_label(),
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
    pub(super) fn timeline_entry_count(&self) -> usize {
        let mut count = 0;
        if !self.current_turn_input.trim().is_empty() {
            count += 1;
        }
        count += self.current_turn_tool_invocations.len();
        count += self.pending_turn_tool_calls.len();
        count.max(1)
    }
}
