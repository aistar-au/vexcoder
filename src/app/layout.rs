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

        // Command sessions get their own entries with session identity.
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
                step_id: session.id,
                lifecycle: StepLifecycle::CommandSession,
                label: format!("{}: {}", session.command, session.status),
                detail,
                session_id: Some(session.id),
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

        // User input echo — step_id 0 is reserved for the user input row.
        if !input_text.trim().is_empty() {
            entries.push(TimelineEntry {
                step_id: 0,
                lifecycle: StepLifecycle::UserInput,
                label: input_text.clone(),
                detail: input_text.clone(),
                session_id: None,
            });
        }

        // Completed tool invocations — step identity carried from the
        // pending call that created them.
        for invocation in tool_invocations {
            let is_error = tool_outcome_is_error(&invocation.outcome);
            entries.push(TimelineEntry {
                step_id: invocation.step_id,
                lifecycle: if is_error {
                    StepLifecycle::Failed
                } else {
                    StepLifecycle::Completed
                },
                label: format!("{}: {}", invocation.name, invocation.outcome),
                detail: format!("Tool: {}\nOutcome: {}", invocation.name, invocation.outcome,),
                session_id: None,
            });
        }

        // In-flight tool calls from pending_turn_tool_calls (task-state owned).
        if has_pending {
            let mut pending_calls: Vec<&PendingTurnToolCall> =
                self.pending_turn_tool_calls.values().collect();
            pending_calls.sort_by_key(|pending| pending.step_id);
            for pending in pending_calls {
                let input_preview = serde_json::to_string_pretty(&pending.input)
                    .unwrap_or_else(|_| pending.input.to_string());
                entries.push(TimelineEntry {
                    step_id: pending.step_id,
                    lifecycle: StepLifecycle::Running,
                    label: format!("{}: running...", pending.name),
                    detail: format!("Tool: {}\nInput:\n{}", pending.name, input_preview),
                    session_id: None,
                });
            }
        }

        entries
    }

    fn task_activity_rows(&self) -> Vec<String> {
        const MAX_ACTIVITY_ROWS: usize = 6;

        if let Some(rows) = self.command_session_rows() {
            return rows
                .into_iter()
                .rev()
                .take(MAX_ACTIVITY_ROWS)
                .rev()
                .collect();
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
            let prefix = if tool_outcome_is_error(&invocation.outcome) {
                "[!]"
            } else {
                "[ok]"
            };
            rows.push(format!(
                "{prefix} {}: {}",
                invocation.name, invocation.outcome
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
        // 6-line dropdown appearance.
        rows.into_iter()
            .rev()
            .take(MAX_ACTIVITY_ROWS)
            .rev()
            .collect()
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
    pub(super) fn task_output_view(&self) -> (String, Vec<String>, OutputScrollAnchor) {
        // If timeline has entries and a valid selection on a tool step
        // (not user input), show inspector detail.
        let entries = self.task_timeline_entries();
        if !entries.is_empty() {
            let idx = self
                .selected_timeline_index
                .min(entries.len().saturating_sub(1));
            if let Some(entry) = entries.get(idx) {
                let is_tool_step = !matches!(entry.lifecycle, StepLifecycle::UserInput);
                if is_tool_step && !entry.detail.is_empty() {
                    let mut rows: Vec<String> =
                        entry.detail.lines().map(ToOwned::to_owned).collect();
                    // Append streaming model response below the inspector
                    // detail when the turn is still in progress.
                    if self.history_state.turn_in_progress && !self.current_turn_response.is_empty()
                    {
                        rows.push(String::new());
                        rows.push("--- model response ---".to_string());
                        rows.extend(self.current_turn_response.lines().map(ToOwned::to_owned));
                    }
                    return ("Inspector".to_string(), rows, OutputScrollAnchor::Top);
                }
            }
        }

        if self.history_state.turn_in_progress {
            let rows = self
                .current_turn_response
                .lines()
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            if rows.is_empty() {
                return (
                    "Transcript".to_string(),
                    vec!["[awaiting model response]".to_string()],
                    OutputScrollAnchor::Bottom,
                );
            }
            return ("Transcript".to_string(), rows, OutputScrollAnchor::Bottom);
        }

        // After turn completes: show enriched paragraph view with tool
        // invocations and model response from the last completed turn.
        if !self.last_turn_tool_invocations.is_empty() || !self.last_turn_response.is_empty() {
            return (
                "Transcript".to_string(),
                self.enriched_paragraph_rows(),
                OutputScrollAnchor::Bottom,
            );
        }

        // No turn data yet — show a welcome hint.
        (
            "Transcript".to_string(),
            vec![
                "Type a prompt below to begin.".to_string(),
                String::new(),
                "The orchestrator will call tools and stream results here.".to_string(),
            ],
            OutputScrollAnchor::Bottom,
        )
    }

    /// Build enriched paragraph rows from the last completed turn.
    ///
    /// Each tool invocation is rendered as a paragraph tree with stable
    /// disclosure markers that the transcript renderer expands to 2/4/6-space
    /// visual paragraphs:
    ///
    /// ```text
    /// [tool] tool_name: brief outcome summary     ← summary (2-space visual)
    /// [detail] detail line 1                      ← phase detail (4-space)
    /// [detail] detail line 2                      ← phase detail (4-space)
    /// [evidence] evidence line 1                  ← evidence (6-space)
    /// [evidence] ... N more lines                 ← truncation hint
    /// ```
    ///
    /// The summary line is self-informative: it includes the tool name, a
    /// compact extract from the outcome, and the completion status so the
    /// operator can scan without expanding detail. Phase-detail rows carry the
    /// structured marker that the renderer turns into 4-space disclosure, and
    /// evidence rows carry the 6-space disclosure marker.
    ///
    /// Followed by the model response text.
    fn enriched_paragraph_rows(&self) -> Vec<String> {
        /// Maximum outcome lines shown at phase-detail level (4-space indent).
        const MAX_PHASE_LINES: usize = 4;
        /// Maximum outcome lines shown at evidence level (6-space indent).
        const MAX_EVIDENCE_LINES: usize = 3;

        let mut rows = Vec::new();

        for invocation in &self.last_turn_tool_invocations {
            if !rows.is_empty() {
                rows.push(String::new());
            }
            let is_error = tool_outcome_is_error(&invocation.outcome);
            let outcome_lines: Vec<&str> = invocation
                .outcome
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect();
            let first_line = outcome_lines.first().copied().unwrap_or("");
            let status_label = if is_error { "failed" } else { "completed" };

            // Summary line: tool name + compact first-line extract.
            if first_line.is_empty() {
                rows.push(format!("[tool] {} ({status_label})", invocation.name));
            } else {
                let brief = compact_outcome_summary(first_line);
                rows.push(format!(
                    "[tool] {}: {} ({status_label})",
                    invocation.name, brief
                ));
            }

            let mut disclosure_lines = Vec::new();
            disclosure_lines.push(format!("status: {status_label}"));
            if !first_line.is_empty() {
                disclosure_lines.push(format!("result: {first_line}"));
            }
            disclosure_lines.extend(outcome_lines.iter().skip(1).map(|line| (*line).to_string()));

            // Phase detail (4-space): early disclosure rows with structured markers.
            for line in disclosure_lines.iter().take(MAX_PHASE_LINES) {
                rows.push(format!("[detail] {line}"));
            }

            // Evidence (6-space): remaining disclosure rows after phase detail, capped.
            if disclosure_lines.len() > MAX_PHASE_LINES {
                let evidence_end = disclosure_lines
                    .len()
                    .min(MAX_PHASE_LINES + MAX_EVIDENCE_LINES);
                for line in &disclosure_lines[MAX_PHASE_LINES..evidence_end] {
                    rows.push(format!("[evidence] {line}"));
                }
                let remaining = disclosure_lines.len().saturating_sub(evidence_end);
                if remaining > 0 {
                    rows.push(format!("[evidence] \u{2026} {} more lines", remaining));
                }
            }
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
        let (output_title, output_rows, output_scroll_anchor) = self.task_output_view();
        let output_scroll_offset = match output_scroll_anchor {
            OutputScrollAnchor::Bottom => self.transcript_scroll_offset,
            OutputScrollAnchor::Top => self.inspector_scroll_offset,
        };

        let input_hint = if let Some(approval) = pending_approval.clone() {
            format!("{approval}\n[y/n/s] ")
        } else if self.command_session_active() {
            "Prompt\nCommand session active. Ctrl+C cancels the running command.".to_string()
        } else {
            "Prompt\nUse `/` for commands, `@path` to inline files, paste large blocks, and Shift+Enter for a newline.".to_string()
        };
        Some(TaskLayoutState {
            task_id: self.current_task.id.clone(),
            status_line: self.status_line(),
            activity_rows,
            timeline_entries,
            selected_step,
            total_steps,
            output_title,
            output_rows,
            output_scroll_offset,
            output_scroll_anchor,
            pending_approval,
            input_hint,
            composer_text: String::new(),
            composer_cursor: 0,
            changed_files: self
                .current_task
                .changed_files
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            follow_mode: self.timeline_follow_mode,
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

/// Produce a compact summary string from the first outcome line.
///
/// Truncates to at most 60 display characters so the summary line stays
/// readable on typical terminal widths without wrapping.
fn compact_outcome_summary(line: &str) -> String {
    const MAX_SUMMARY_WIDTH: usize = 60;
    let trimmed = line.trim();
    if trimmed.len() <= MAX_SUMMARY_WIDTH {
        return trimmed.to_string();
    }
    let mut end = trimmed.floor_char_boundary(MAX_SUMMARY_WIDTH);
    // Snap to a word boundary when possible.
    if let Some(space_pos) = trimmed[..end].rfind(' ') {
        if space_pos > MAX_SUMMARY_WIDTH / 2 {
            end = space_pos;
        }
    }
    format!("{}\u{2026}", &trimmed[..end])
}

fn tool_outcome_is_error(outcome: &str) -> bool {
    let lowered = outcome.trim().to_ascii_lowercase();
    lowered.starts_with("error")
        || lowered.starts_with("failed")
        || lowered.contains("denied")
        || lowered.starts_with("cancelled")
        || lowered.starts_with("canceled")
}

#[cfg(test)]
mod tests {
    use super::{compact_outcome_summary, tool_outcome_is_error, TuiMode};
    use crate::turn_evidence::ToolInvocationSummary;

    #[test]
    fn short_outcome_preserved() {
        assert_eq!(compact_outcome_summary("ok"), "ok");
        assert_eq!(compact_outcome_summary("42 lines"), "42 lines");
    }

    #[test]
    fn long_outcome_truncated_at_word_boundary() {
        let long = "this is a long outcome line that exceeds sixty characters and should be truncated at a word boundary";
        let result = compact_outcome_summary(long);
        assert!(
            result.len() <= 65,
            "truncated result must be compact: got {result:?}"
        );
        assert!(
            result.ends_with('\u{2026}'),
            "truncated result must end with ellipsis: got {result:?}"
        );
        assert!(
            result.contains("this is a long"),
            "truncated result must preserve the start: got {result:?}"
        );
    }

    #[test]
    fn whitespace_trimmed() {
        assert_eq!(compact_outcome_summary("  ok  "), "ok");
    }

    #[test]
    fn error_outcome_classifier_treats_denials_as_failures() {
        assert!(tool_outcome_is_error("permission denied"));
        assert!(tool_outcome_is_error("cancelled by user"));
        assert!(!tool_outcome_is_error("ok"));
    }

    #[test]
    fn enriched_paragraph_rows_emit_structured_markers() {
        let mut mode = TuiMode::new();
        mode.last_turn_tool_invocations = vec![ToolInvocationSummary {
            step_id: 1,
            name: "read_file".to_string(),
            outcome: [
                "42 lines read from src/main.rs",
                "scope: src/main.rs",
                "command: read_file src/main.rs",
                "preview: fn main() {}",
                "preview: println!(\"hello\");",
            ]
            .join("\n"),
        }];
        mode.last_turn_response = "Done.".to_string();

        assert_eq!(
            mode.enriched_paragraph_rows(),
            vec![
                "[tool] read_file: 42 lines read from src/main.rs (completed)".to_string(),
                "[detail] status: completed".to_string(),
                "[detail] result: 42 lines read from src/main.rs".to_string(),
                "[detail] scope: src/main.rs".to_string(),
                "[detail] command: read_file src/main.rs".to_string(),
                "[evidence] preview: fn main() {}".to_string(),
                "[evidence] preview: println!(\"hello\");".to_string(),
                String::new(),
                "Done.".to_string(),
            ]
        );
    }

    #[test]
    fn enriched_paragraph_rows_cap_evidence_with_hint() {
        let mut mode = TuiMode::new();
        mode.last_turn_tool_invocations = vec![ToolInvocationSummary {
            step_id: 2,
            name: "write_file".to_string(),
            outcome: [
                "permission denied",
                "target: /tmp/demo.txt",
                "command: write_file /tmp/demo.txt",
                "result: denied",
                "stderr: line 1",
                "stderr: line 2",
                "stderr: line 3",
                "stderr: line 4",
            ]
            .join("\n"),
        }];

        assert_eq!(
            mode.enriched_paragraph_rows(),
            vec![
                "[tool] write_file: permission denied (failed)".to_string(),
                "[detail] status: failed".to_string(),
                "[detail] result: permission denied".to_string(),
                "[detail] target: /tmp/demo.txt".to_string(),
                "[detail] command: write_file /tmp/demo.txt".to_string(),
                "[evidence] result: denied".to_string(),
                "[evidence] stderr: line 1".to_string(),
                "[evidence] stderr: line 2".to_string(),
                "[evidence] … 2 more lines".to_string(),
            ]
        );
    }
}
