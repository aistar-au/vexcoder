use super::*;

impl TuiMode {
    pub(super) fn command_session_rows(&self) -> Option<Vec<String>> {
        if self.command_sessions.is_empty() {
            return None;
        }
        let mut rows = Vec::new();
        for (i, session) in self.command_sessions.iter().enumerate() {
            if i > 0 {
                rows.push(String::new());
            }
            rows.push(format!("command: {}", session.command));
            rows.push(format!(
                "pid    : {}",
                session
                    .pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "pending".to_string())
            ));
            rows.push(format!("status : {}", session.status));
        }
        Some(rows)
    }

    fn task_activity_rows(&self) -> Vec<String> {
        const MAX_ACTIVITY_ROWS: usize = 6;

        if let Some(rows) = self.command_session_rows() {
            return rows.into_iter().rev().take(MAX_ACTIVITY_ROWS).rev().collect();
        }

        let mut rows = Vec::new();

        // Prompt prefix for the current turn input.
        if !self.current_turn_input.trim().is_empty() {
            rows.push(format!("> {}", self.current_turn_input));
        }

        // Completed tool invocations — prefixed to match render_task_layout
        // style markers ([ok] / [!]).
        for invocation in &self.current_turn_tool_invocations {
            let prefix = if invocation.outcome.starts_with("error")
                || invocation.outcome.starts_with("failed")
                || invocation.outcome.starts_with("Error")
            {
                "[!]"
            } else {
                "[ok]"
            };
            rows.push(format!("{prefix} {}: {}", invocation.name, invocation.outcome));
        }

        // In-flight tool calls (model sent the call, result not yet received).
        // These are shown with the [->] running marker so the user can see the
        // orchestrator is still active — without this, the activity pane goes
        // blank while a long-running tool is executing.
        for pending in self.pending_turn_tool_calls.values() {
            rows.push(format!("[->] {}: running…", pending.name));
        }

        if rows.is_empty() {
            return self
                .history_state
                .lines
                .iter()
                .rev()
                .filter(|line| !line.trim().is_empty())
                .take(MAX_ACTIVITY_ROWS)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
        }

        // Clamp to the most-recent MAX_ACTIVITY_ROWS lines for a stable
        // 6-line dropdown appearance — older completed steps scroll off the
        // top naturally.
        rows.into_iter().rev().take(MAX_ACTIVITY_ROWS).rev().collect()
    }

    fn task_output_rows(&self) -> Vec<String> {
        if self.history_state.turn_in_progress {
            let rows = self
                .current_turn_response
                .lines()
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            if rows.is_empty() {
                return vec!["[awaiting model response]".to_string()];
            }
            return rows;
        }

        self.history_state
            .lines
            .iter()
            .rev()
            .take(24)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    pub fn task_layout_state(&self) -> Option<TaskLayoutState> {
        if !self.history_state.turn_in_progress && !self.overlay_active() {
            return None;
        }

        let pending_approval = if self.overlay_state.pending_patch_approval.is_some() {
            Some("ApplyPatch".to_string())
        } else if self.overlay_state.pending_resume_selection.is_some() {
            Some("Resume saved task\n[type 1-5 or n to cancel]".to_string())
        } else {
            self.overlay_state.pending_approval.as_ref().map(|pending| {
                summarize_tool_approval_context(&pending.tool_name, &pending.input_preview)
            })
        };

        let activity_rows = self.task_activity_rows();
        let input_hint = if let Some(approval) = pending_approval.clone() {
            format!("{approval}\n[y/n/s] ")
        } else if self.command_session_active() {
            "[command session active \u{2014} Ctrl+C to cancel]".to_string()
        } else {
            "> ".to_string()
        };
        Some(TaskLayoutState {
            task_id: self.current_task.id.clone(),
            status_line: self.status_line(),
            activity_rows,
            output_rows: self.task_output_rows(),
            pending_approval,
            input_hint,
            changed_files: self
                .current_task
                .changed_files
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        })
    }

    pub(super) fn registered_slash_command(
        input: &str,
    ) -> Option<(&'static SlashCommandSpec, &str)> {
        let trimmed = input.trim();
        SLASH_COMMANDS
            .iter()
            .find_map(|spec| spec.pattern.parse(trimmed).map(|args| (spec, args)))
    }

    pub(super) fn registered_custom_command<'a>(
        &'a self,
        input: &'a str,
    ) -> Option<(&'a CustomCommand, &'a str)> {
        let trimmed = input.trim();
        let raw = trimmed.strip_prefix('/')?;
        let (name, args) = raw
            .find(char::is_whitespace)
            .map(|index| (&raw[..index], raw[index..].trim()))
            .unwrap_or((raw, ""));
        self.custom_commands
            .iter()
            .find(|command| command.name == name)
            .map(|command| (command, args))
    }

    pub(super) fn is_reentrant_edit_command(input: &str) -> bool {
        Self::registered_slash_command(input)
            .map(|(spec, _)| matches!(spec.id, SlashCommandId::Edit | SlashCommandId::Fix))
            .unwrap_or(false)
    }
}
