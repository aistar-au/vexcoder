use super::*;
use crate::app::scroll::apply_bounded_scroll;
use crate::tool_preview::{preview_tool_input, ToolPreviewStyle};

#[cfg(test)]
use crossterm::event::{Event, KeyCode, KeyModifiers};

impl TuiMode {
    pub(super) fn pending_tool_step_id(&self, tool_name: &str, input_preview: &str) -> Option<u64> {
        let entries = &self.task_doc.active_turn.as_ref()?.entries;

        // Prefer an exact match on both tool name and input preview so that
        // repeated calls to the same tool (e.g. multiple `read_file` with
        // different paths) are attributed to the correct step.
        let exact_match = entries.iter().find_map(|e| {
            if let crate::runtime::TurnEntry::ToolCall {
                step_id,
                name,
                input,
                ..
            } = e
            {
                if name == tool_name {
                    let entry_preview = preview_tool_input(
                        name,
                        input,
                        ToolPreviewStyle::Structured,
                        crate::edit_diff::DEFAULT_EDIT_DIFF_CONTEXT_LINES,
                    );
                    if entry_preview == input_preview {
                        return Some(*step_id);
                    }
                }
            }
            None
        });

        exact_match.or_else(|| {
            // Fall back to the first name-only match when no preview aligns
            // (e.g. approval arrives before the entry's input is fully parsed).
            entries
                .iter()
                .filter_map(|e| {
                    if let crate::runtime::TurnEntry::ToolCall { step_id, name, .. } = e {
                        if name == tool_name {
                            return Some(*step_id);
                        }
                    }
                    None
                })
                .min()
        })
    }

    pub(super) fn mark_tool_step_approved(&mut self, step_id: Option<u64>) {
        if let Some(step_id) = step_id {
            self.overlay_state.approved_tool_steps.insert(step_id);
        }
        self.set_task_status(TaskStatus::Running);
    }

    pub(super) fn resolve_pending_approval(&mut self, approved: bool, ctx: &RuntimeContext) {
        if let Some(pending) = self.overlay_state.pending_approval.take() {
            if approved {
                self.mark_tool_step_approved(pending.step_id);
            }
            // On denial, skip the intermediate Running persist — the edit
            // loop will set Cancelled via set_task_status() immediately,
            // closing the crash-resume race window (Finding 7).
            match pending.action {
                PendingApprovalAction::Tool(response_tx) => {
                    let _ = response_tx.send(approved);
                }
                PendingApprovalAction::InlineCommand(command) => {
                    if approved {
                        self.start_command_session(command.command, ctx);
                    }
                }
            }
        }
    }

    pub(super) fn handle_approval_input(&mut self, input: &str, ctx: &mut RuntimeContext) {
        if self.overlay_state.pending_resume_selection.is_some() {
            self.handle_resume_selection_input(input, ctx);
            return;
        }
        let context = self
            .overlay_state
            .pending_approval
            .as_ref()
            .map(|p| summarize_tool_approval_context(&p.tool_name, &p.input_preview))
            .unwrap_or_else(|| "unknown".to_string());
        match parse_approval_selection(input) {
            Some(ApprovalSelection::ApproveOnce) => {
                self.push_document_notice(
                    format!("[tool approval accepted once: {context}]"),
                    crate::runtime::NoticeSeverity::Info,
                );
                self.resolve_pending_approval(true, ctx);
            }
            Some(ApprovalSelection::ApproveSession) => {
                self.overlay_state.auto_approve_session = true;
                self.push_document_notice(
                    format!("[tool approval enabled for session: {context}]"),
                    crate::runtime::NoticeSeverity::Info,
                );
                self.resolve_pending_approval(true, ctx);
            }
            Some(ApprovalSelection::Deny) => {
                self.push_document_notice(
                    format!("[tool approval denied: {context}]"),
                    crate::runtime::NoticeSeverity::Info,
                );
                self.resolve_pending_approval(false, ctx);
            }
            None => {
                self.push_document_notice(
                    "[invalid selection, expected 1/2/3]".to_string(),
                    crate::runtime::NoticeSeverity::Info,
                );
            }
        }
    }

    pub(super) fn resolve_pending_patch_approval(&mut self, approved: bool) {
        if let Some(mut pending) = self.overlay_state.pending_patch_approval.take() {
            if let Some(tx) = pending.response_tx.take() {
                let _ = tx.send(approved);
            }
            let decision = if approved { "accepted" } else { "denied" };
            self.push_document_notice(
                format!("[patch approval {decision}]"),
                crate::runtime::NoticeSeverity::Info,
            );
        }
    }

    pub(super) fn apply_patch_overlay_scroll_action(&mut self, action: ScrollAction) {
        if let Some(pending) = self.overlay_state.pending_patch_approval.as_mut() {
            let max = pending.patch_preview.lines().count().saturating_sub(1);
            apply_bounded_scroll(&mut pending.scroll_offset, action, max);
        }
    }

    pub(super) fn handle_patch_overlay_input(&mut self, input: &str) {
        if self.overlay_state.pending_patch_approval.is_none() {
            return;
        }

        match parse_approval_selection(input) {
            Some(ApprovalSelection::ApproveOnce) => self.resolve_pending_patch_approval(true),
            Some(ApprovalSelection::Deny) => self.resolve_pending_patch_approval(false),
            Some(ApprovalSelection::ApproveSession) | None => {}
        }
    }

    pub(super) fn prompt_resume_selection(&mut self, entries: Vec<ResumeTaskEntry>) {
        self.push_document_notice(
            "[resume] choose a task to resume:".to_string(),
            crate::runtime::NoticeSeverity::Info,
        );
        for (index, entry) in entries.iter().enumerate() {
            self.push_document_notice(
                format!("  {}. {} ({})", index + 1, entry.id, entry.status),
                crate::runtime::NoticeSeverity::Info,
            );
        }
        self.push_document_notice(
            "[resume] type 1-5 to select or n to cancel".to_string(),
            crate::runtime::NoticeSeverity::Info,
        );
        self.overlay_state.pending_resume_selection = Some(PendingResumeSelection { entries });
    }

    pub(super) fn handle_resume_selection_input(&mut self, input: &str, ctx: &mut RuntimeContext) {
        let trimmed = input.trim();
        if matches!(trimmed.to_ascii_lowercase().as_str(), "n" | "no" | "esc") {
            self.overlay_state.pending_resume_selection = None;
            self.push_document_notice(
                "[resume] cancelled".to_string(),
                crate::runtime::NoticeSeverity::Info,
            );
            return;
        }

        let Some(selection) = trimmed.parse::<usize>().ok() else {
            self.push_document_notice(
                "[resume] invalid selection, expected 1-5 or n".to_string(),
                crate::runtime::NoticeSeverity::Info,
            );
            return;
        };

        let Some(entry) = self
            .overlay_state
            .pending_resume_selection
            .as_ref()
            .and_then(|pending| pending.entries.get(selection.saturating_sub(1)))
            .cloned()
        else {
            self.push_document_notice(
                "[resume] invalid selection, expected 1-5 or n".to_string(),
                crate::runtime::NoticeSeverity::Info,
            );
            return;
        };

        self.overlay_state.pending_resume_selection = None;
        match TaskState::load(&entry.dir, &entry.id) {
            Ok(state) => self.apply_resumed_task(state, ctx),
            Err(_) => {
                self.push_document_notice(
                    format!("[resume: task '{}' not found]", entry.id),
                    crate::runtime::NoticeSeverity::Warning,
                );
            }
        }
    }
}

pub(super) fn summarize_tool_approval_context(tool_name: &str, input_preview: &str) -> String {
    let mut path: Option<&str> = None;
    let mut summary_line: Option<&str> = None;

    for line in input_preview.lines().take(8) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if path.is_none() && trimmed.starts_with("path:") {
            path = Some(trimmed);
            continue;
        }
        if summary_line.is_none()
            && (trimmed.starts_with("change:") || trimmed.starts_with("content:"))
        {
            summary_line = Some(trimmed);
            continue;
        }
        if summary_line.is_none() {
            summary_line = Some(trimmed);
        }
    }

    match (path, summary_line) {
        (Some(path), Some(summary)) => format!("{tool_name} {path} {summary}"),
        (Some(path), None) => format!("{tool_name} {path}"),
        (None, Some(summary)) => format!("{tool_name} {summary}"),
        (None, None) => tool_name.to_string(),
    }
}

pub(super) fn parse_approval_selection(input: &str) -> Option<ApprovalSelection> {
    let normalized = input.trim().to_lowercase();
    match normalized.as_str() {
        "1" | "y" | "yes" => Some(ApprovalSelection::ApproveOnce),
        "2" | "a" | "always" => Some(ApprovalSelection::ApproveSession),
        "3" | "n" | "no" | "esc" => Some(ApprovalSelection::Deny),
        _ => None,
    }
}

#[cfg(test)]
pub(super) fn overlay_event_to_user_input(event: Event) -> Option<UserInputEvent> {
    match event {
        Event::Key(key) => match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Interrupt)
            }
            KeyCode::Esc => Some(UserInputEvent::Text("esc".to_string())),
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                Some(UserInputEvent::Text(ch.to_string()))
            }
            _ => None,
        },
        Event::Paste(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(UserInputEvent::Text(trimmed.to_string()))
            }
        }
        _ => None,
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RenderPass {
    Header,
    History,
    Input,
    Overlay,
}

#[cfg(test)]
pub(super) fn render_pass_order(mode: &TuiMode) -> Vec<RenderPass> {
    let mut order = vec![RenderPass::Header, RenderPass::History, RenderPass::Input];
    if mode.overlay_active() {
        order.push(RenderPass::Overlay);
    }
    order
}
