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

    /// Derive structured timeline entries from canonical task state.
    ///
    /// Each entry carries lifecycle, label, and inspector detail so the
    /// renderer can highlight the selected step and show its content in
    /// the output/inspector pane.
    ///
    /// When no turn is in progress, entries are derived from the last
    /// completed turn so the four-region layout remains populated.
    fn task_timeline_entries(&self) -> Vec<TimelineEntry> {
        let mut entries = Vec::new();

        // Command sessions get their own entries.
        for session in &self.command_sessions {
            let detail = format!(
                "command: {}\npid: {}\nstatus: {}",
                session.command,
                session
                    .pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "pending".to_string()),
                session.status,
            );
            entries.push(TimelineEntry {
                lifecycle: StepLifecycle::CommandSession,
                label: format!("{}: {}", session.command, session.status),
                detail,
            });
        }
        if !entries.is_empty() {
            return entries;
        }

        // Determine which turn data to display: current (in-progress) or
        // last completed turn for persistent display.
        let (input_text, tool_invocations, has_pending) = if self.history_state.turn_in_progress
            || !self.current_turn_input.trim().is_empty()
            || !self.current_turn_tool_invocations.is_empty()
            || !self.pending_turn_tool_calls.is_empty()
        {
            (
                &self.current_turn_input,
                &self.current_turn_tool_invocations,
                true,
            )
        } else {
            (
                &self.last_turn_input_display,
                &self.last_turn_tool_invocations,
                false,
            )
        };

        // User input echo.
        if !input_text.trim().is_empty() {
            entries.push(TimelineEntry {
                lifecycle: StepLifecycle::UserInput,
                label: input_text.clone(),
                detail: input_text.clone(),
            });
        }

        // Completed tool invocations — derived from canonical task state.
        for invocation in tool_invocations {
            let is_error = tool_outcome_prefix(&invocation.outcome) == "[!]";
            entries.push(TimelineEntry {
                lifecycle: if is_error {
                    StepLifecycle::Failed
                } else {
                    StepLifecycle::Completed
                },
                label: format!("{}: {}", invocation.name, invocation.outcome),
                detail: format!("Tool: {}\nOutcome: {}", invocation.name, invocation.outcome,),
            });
        }

        // In-flight tool calls from pending_turn_tool_calls (task-state owned).
        // Sort by key for stable iteration order across frames.
        if has_pending {
            let mut pending_keys: Vec<&String> = self.pending_turn_tool_calls.keys().collect();
            pending_keys.sort();
            for key in pending_keys {
                let pending = &self.pending_turn_tool_calls[key];
                let input_preview = serde_json::to_string_pretty(&pending.input)
                    .unwrap_or_else(|_| pending.input.to_string());
                entries.push(TimelineEntry {
                    lifecycle: StepLifecycle::Running,
                    label: format!("{}: running...", pending.name),
                    detail: format!("Tool: {}\nInput:\n{}", pending.name, input_preview),
                });
            }
        }

        entries
    }

    fn task_activity_rows(&self) -> Vec<String> {
        if let Some(rows) = self.command_session_rows() {
            return rows;
        }

        let mut rows = Vec::new();

        // Determine source: current turn or last completed turn.
        let (input_text, tool_invocations) = if self.history_state.turn_in_progress
            || !self.current_turn_input.trim().is_empty()
            || !self.current_turn_tool_invocations.is_empty()
            || !self.pending_turn_tool_calls.is_empty()
        {
            (
                &self.current_turn_input,
                &self.current_turn_tool_invocations,
            )
        } else if !self.last_turn_tool_invocations.is_empty()
            || !self.last_turn_input_display.is_empty()
        {
            (
                &self.last_turn_input_display,
                &self.last_turn_tool_invocations,
            )
        } else {
            (
                &self.current_turn_input,
                &self.current_turn_tool_invocations,
            )
        };

        // Prompt prefix for the turn input.
        if !input_text.trim().is_empty() {
            rows.push(format!("> {}", input_text));
        }

        // Completed tool invocations — prefixed to match render_task_layout
        // style markers ([ok] / [!]).
        for invocation in tool_invocations {
            rows.push(format!(
                "{} {}: {}",
                tool_outcome_prefix(&invocation.outcome),
                invocation.name,
                invocation.outcome
            ));
        }

        // In-flight tool calls (model sent the call, result not yet received).
        // Sort by key for stable order.
        let mut pending_keys: Vec<&String> = self.pending_turn_tool_calls.keys().collect();
        pending_keys.sort();
        for key in &pending_keys {
            let pending = &self.pending_turn_tool_calls[*key];
            rows.push(format!("[->] {}: running...", pending.name));
        }

        if rows.is_empty() {
            return self
                .history_state
                .lines
                .iter()
                .filter(|line| !line.trim().is_empty())
                .cloned()
                .collect();
        }

        rows
    }

    /// Derive output/inspector rows for the output pane.
    ///
    /// Rendering strategy:
    /// - During an active turn with timeline entries: inspector detail for the
    ///   selected tool step, with streaming model response appended below.
    /// - During an active turn without tool steps: streaming model response.
    /// - After a completed turn: enriched paragraph view showing each tool
    ///   invocation as a paragraph followed by the model response.
    /// - Before any turn: welcome hint.
    fn task_output_rows(&self) -> Vec<String> {
        if self.history_state.turn_in_progress {
            return self.active_turn_transcript_rows();
        }

        // After turn completes: show enriched paragraph view with tool
        // invocations and model response from the last completed turn.
        if !self.last_turn_tool_invocations.is_empty() || !self.last_turn_response.is_empty() {
            return self.enriched_paragraph_rows();
        }

        // No turn data yet — show a welcome hint.
        vec![
            "Type a prompt below to begin.".to_string(),
            String::new(),
            "The orchestrator will call tools and stream results here.".to_string(),
        ]
    }

    /// Build enriched paragraph rows from the last completed turn.
    ///
    /// Each tool invocation is rendered as a short paragraph:
    ///   tool_name outcome
    ///
    /// Followed by the model response text.
    fn enriched_paragraph_rows(&self) -> Vec<String> {
        let mut rows = Vec::new();

        for invocation in &self.last_turn_tool_invocations {
            self.push_tool_paragraph(
                &mut rows,
                invocation.name.as_str(),
                invocation.outcome.as_str(),
            );
        }

        if !self.last_turn_response.is_empty() {
            if !rows.is_empty() {
                rows.push(String::new());
            }
            for line in self.last_turn_response.lines() {
                rows.push(line.to_string());
            }
        }

        if rows.is_empty() {
            rows.push("Turn completed.".to_string());
        }

        rows
    }

    fn active_turn_transcript_rows(&self) -> Vec<String> {
        let mut rows = Vec::new();

        for invocation in &self.current_turn_tool_invocations {
            self.push_tool_paragraph(
                &mut rows,
                invocation.name.as_str(),
                invocation.outcome.as_str(),
            );
        }

        let mut pending_keys: Vec<&String> = self.pending_turn_tool_calls.keys().collect();
        pending_keys.sort();
        for key in pending_keys {
            let pending = &self.pending_turn_tool_calls[key];
            self.push_tool_paragraph(&mut rows, pending.name.as_str(), RUNNING_OUTCOME);
        }

        if !self.current_turn_response.is_empty() {
            if !rows.is_empty() {
                rows.push(String::new());
            }
            rows.extend(self.current_turn_response.lines().map(ToOwned::to_owned));
        }

        if rows.is_empty() {
            rows.push("[awaiting model response]".to_string());
        }

        rows
    }

    fn push_tool_paragraph(&self, rows: &mut Vec<String>, name: &str, outcome: &str) {
        if !rows.is_empty() {
            rows.push(String::new());
        }

        rows.push(format!("{} {name}", tool_outcome_prefix(outcome)));
        for line in outcome.lines() {
            rows.push(format!("    {line}"));
        }
    }

    pub fn task_layout_state(&self) -> Option<TaskLayoutState> {
        // Always return the four-region layout. The task-state control surface
        // is persistent — it is never yielded back to the transcript-only
        // three-pane view between tool calls or after a turn completes.
        // This follows ADR-031: the operator surface derives from canonical
        // task state and remains visible at all times.

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
        let timeline_entries = self.task_timeline_entries();
        let total_steps = timeline_entries.len();
        let selected_step = self
            .selected_timeline_index
            .min(total_steps.saturating_sub(1));

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
            timeline_entries,
            selected_step,
            total_steps,
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

fn tool_outcome_prefix(outcome: &str) -> &'static str {
    if outcome == RUNNING_OUTCOME {
        return "[->]";
    }

    if starts_with_ascii_case_insensitive(outcome, "error")
        || starts_with_ascii_case_insensitive(outcome, "failed")
    {
        "[!]"
    } else {
        "[ok]"
    }
}

const RUNNING_OUTCOME: &str = "running...";

fn starts_with_ascii_case_insensitive(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
}
