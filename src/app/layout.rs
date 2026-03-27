use super::*;

struct TaskStepView {
    step_id: u64,
    lifecycle: StepLifecycle,
    label: String,
    detail: String,
    session_id: Option<u64>,
}

impl TuiMode {
    /// Derive structured timeline entries from canonical task state.
    ///
    /// Implements ADR-031 Batch B by deriving both the structured timeline
    /// and the legacy activity summary from the same task-owned step views.
    ///
    /// Each derived entry carries lifecycle, label, and inspector detail so
    /// the renderer can highlight the selected step and show its content in
    /// the output/inspector pane.
    ///
    /// When no turn is in progress, entries are derived from the last
    /// completed turn so the four-region layout remains populated.
    fn task_timeline_entries_from(steps: &[TaskStepView]) -> Vec<TimelineEntry> {
        steps
            .iter()
            .map(|step| TimelineEntry {
                step_id: step.step_id,
                lifecycle: step.lifecycle.clone(),
                label: step.label.clone(),
                detail: step.detail.clone(),
                session_id: step.session_id,
            })
            .collect()
    }

    fn task_step_views(&self) -> Vec<TaskStepView> {
        let mut entries = Vec::new();

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
            entries.push(TaskStepView {
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
            entries.push(TaskStepView {
                step_id: invocation.step_id,
                lifecycle: if is_error {
                    StepLifecycle::Failed
                } else {
                    StepLifecycle::Completed
                },
                label: timeline_label_for_invocation(invocation),
                detail: format!("Tool: {}\nOutcome: {}", invocation.name, invocation.outcome,),
                session_id: None,
            });
        }

        // Pending tool calls from pending_turn_tool_calls (task-state owned).
        if has_pending {
            let mut pending_calls: Vec<&PendingTurnToolCall> =
                self.pending_turn_tool_calls.values().collect();
            pending_calls.sort_by_key(|pending| pending.step_id);
            let awaiting_step_id = self
                .overlay_state
                .pending_approval
                .as_ref()
                .and_then(|pending| pending.step_id);
            for pending in pending_calls {
                let input_preview = serde_json::to_string_pretty(&pending.input)
                    .unwrap_or_else(|_| pending.input.to_string());
                let lifecycle = if awaiting_step_id == Some(pending.step_id) {
                    StepLifecycle::AwaitingApproval
                } else if self
                    .overlay_state
                    .approved_tool_steps
                    .contains(&pending.step_id)
                {
                    StepLifecycle::Approved
                } else {
                    StepLifecycle::Running
                };
                let status = match lifecycle {
                    StepLifecycle::AwaitingApproval => "awaiting approval",
                    StepLifecycle::Approved => "approved",
                    _ => "running...",
                };
                entries.push(TaskStepView {
                    step_id: pending.step_id,
                    lifecycle,
                    label: format!("{}: {}", pending.name, status),
                    detail: format!("Tool: {}\nInput:\n{}", pending.name, input_preview),
                    session_id: None,
                });
            }
        }

        // Command sessions remain visible alongside the rest of the current
        // turn timeline instead of replacing it entirely.
        for session in &self.command_sessions {
            let pid = session
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "pending".to_string());
            entries.push(TaskStepView {
                step_id: session.id,
                lifecycle: StepLifecycle::CommandSession,
                label: format!("{}: {}", session.command, session.status),
                detail: format!(
                    "command: {}\npid: {}\nstatus: {}",
                    session.command, pid, session.status,
                ),
                session_id: Some(session.id),
            });
        }

        entries
    }

    /// Derive output/inspector rows for the output pane.
    ///
    /// Rendering strategy:
    /// - While timeline follow mode is active: accumulated transcript rows so
    ///   each new server response appends at the bottom instead of replacing
    ///   the prior view.
    /// - After manual timeline navigation (follow mode off): inspector detail
    ///   for the selected tool step, with streaming model response appended
    ///   below.
    /// - Before any turn: welcome hint.
    pub(super) fn task_output_view(&self) -> (String, Vec<String>, OutputScrollAnchor) {
        let steps = self.task_step_views();
        let entries = Self::task_timeline_entries_from(&steps);
        self.task_output_view_with(&entries)
    }

    fn task_output_view_with(
        &self,
        entries: &[TimelineEntry],
    ) -> (String, Vec<String>, OutputScrollAnchor) {
        // Keep the output pane on the accumulated transcript while follow mode
        // is active so prior server responses scroll upward instead of being
        // replaced by the latest inspector view. Manual timeline navigation can
        // still switch the pane into inspector mode.
        if !self.timeline_follow_mode && !entries.is_empty() {
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

        let transcript_rows = self.transcript_display_rows();
        if !transcript_rows.is_empty() {
            return (
                "Transcript".to_string(),
                transcript_rows,
                OutputScrollAnchor::Bottom,
            );
        }

        // No turn data yet — leave the transcript blank so the surface starts
        // directly from the prompt edge.
        (
            "Transcript".to_string(),
            Vec::new(),
            OutputScrollAnchor::Bottom,
        )
    }

    fn transcript_display_rows(&self) -> Vec<String> {
        let mut rows = Vec::new();
        for line in &self.history_state.lines {
            if line.is_empty() {
                rows.push(String::new());
            } else {
                rows.extend(line.lines().map(ToOwned::to_owned));
            }
        }

        if self.history_state.turn_in_progress
            && !self.history_state.cancel_pending
            && rows.last().is_some_and(|line| line.is_empty())
        {
            if self.current_turn_response.is_empty() {
                if let Some(last) = rows.last_mut() {
                    *last = "[awaiting model response]".to_string();
                }
            } else {
                rows.pop();
                let mut response_rows = self
                    .current_turn_response
                    .lines()
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>();
                if let Some(last) = response_rows.last_mut() {
                    last.push('▌');
                }
                rows.extend(response_rows);
            }
        } else if self.history_state.turn_in_progress
            && !self.history_state.cancel_pending
            && !self.current_turn_response.is_empty()
            && rows.last().is_some_and(|line| !line.ends_with('▌'))
        {
            if let Some(last) = rows.last_mut() {
                last.push('▌');
            }
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

        let steps = self.task_step_views();
        let timeline_entries = Self::task_timeline_entries_from(&steps);
        let total_steps = timeline_entries.len();
        let selected_step = self
            .selected_timeline_index
            .min(total_steps.saturating_sub(1));
        let (output_title, output_rows, output_scroll_anchor) =
            self.task_output_view_with(&timeline_entries);
        let output_scroll_offset = match output_scroll_anchor {
            OutputScrollAnchor::Bottom => self.transcript_scroll_offset,
            OutputScrollAnchor::Top => self.inspector_scroll_offset,
        };

        let input_hint = if let Some(approval) = pending_approval.clone() {
            format!("{approval}\n[y/n/s] ")
        } else if self.command_session_active() {
            "Prompt\nsubmit: / commands  @ files  ! shell  Ctrl+C cancels".to_string()
        } else {
            "Prompt\nsubmit: / commands  @ files  ! shell".to_string()
        };
        Some(TaskLayoutState {
            task_id: self.current_task.id.clone(),
            status_line: self.status_line(),
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
            composer_focused: self.composer_is_focused(),
            changed_files: self
                .current_task
                .changed_files
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            follow_mode: self.timeline_follow_mode,
            picker_overlay: vec![],
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

fn timeline_label_for_invocation(invocation: &ToolInvocationSummary) -> String {
    let is_error = tool_outcome_is_error(&invocation.outcome);
    let status_label = if is_error { "failed" } else { "completed" };
    let first_line = invocation
        .outcome
        .lines()
        .map(str::trim_end)
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let result_summary = if first_line.is_empty() {
        status_label.to_string()
    } else {
        compact_outcome_summary(first_line)
    };

    if let Some(target_summary) = tool_target_summary(first_line) {
        format!(
            "{} · {} · {}",
            invocation.name, target_summary, status_label
        )
    } else if result_summary == status_label {
        format!("{} · {}", invocation.name, status_label)
    } else {
        format!(
            "{} · {} · {}",
            invocation.name, result_summary, status_label
        )
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
fn tool_scope_detail(tool_name: &str) -> String {
    builtin_tool_summaries()
        .into_iter()
        .find(|tool| tool.name == tool_name)
        .map(|tool| tool.description)
        .unwrap_or_else(|| "Tool invocation recorded in the completed turn.".to_string())
}

fn tool_target_summary(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lowered = trimmed.to_ascii_lowercase();
    for marker in [" from ", " to ", " in ", " at ", " into ", " on "] {
        if let Some(index) = lowered.find(marker) {
            if let Some(candidate) = first_pathish_token(&trimmed[index + marker.len()..]) {
                return Some(candidate);
            }
        }
    }

    first_pathish_token(trimmed)
}

fn first_pathish_token(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|token| {
        let candidate = token.trim_matches(|ch: char| {
            matches!(
                ch,
                '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':'
            )
        });
        if candidate.is_empty() {
            return None;
        }
        let looks_pathish = candidate.contains('/')
            || candidate.contains('\\')
            || candidate
                .rsplit_once('.')
                .map(|(stem, ext)| !stem.is_empty() && !ext.is_empty())
                .unwrap_or(false);
        looks_pathish.then(|| candidate.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::{
        compact_outcome_summary, timeline_label_for_invocation, tool_outcome_is_error,
        tool_scope_detail, tool_target_summary,
    };
    use crate::app::ToolInvocationSummary;

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
        assert!(tool_outcome_is_error("canceled by user"));
        assert!(!tool_outcome_is_error("ok"));
    }

    #[test]
    fn scope_detail_prefers_builtin_tool_description() {
        assert_eq!(
            tool_scope_detail("read_file"),
            "Read file content from an explicit non-empty path. For repository overviews, use list_files or codebase_search first. For large files, use offset and limit to read specific line ranges instead of loading the entire file."
        );
    }

    #[test]
    fn scope_detail_falls_back_for_unknown_tools() {
        assert_eq!(
            tool_scope_detail("custom_tool"),
            "Tool invocation recorded in the completed turn."
        );
    }

    #[test]
    fn target_summary_extracts_path_after_common_prepositions() {
        assert_eq!(
            tool_target_summary("42 lines read from src/main.rs"),
            Some("src/main.rs".to_string())
        );
        assert_eq!(
            tool_target_summary("wrote 17 bytes to Cargo.toml"),
            Some("Cargo.toml".to_string())
        );
    }

    #[test]
    fn target_summary_falls_back_to_first_pathish_token() {
        assert_eq!(
            tool_target_summary("updated src/ui/render.rs successfully"),
            Some("src/ui/render.rs".to_string())
        );
        assert_eq!(tool_target_summary("permission denied"), None);
    }

    #[test]
    fn timeline_label_prefers_target_and_terminal_status() {
        let invocation = ToolInvocationSummary {
            step_id: 1,
            name: "read_file".to_string(),
            outcome: "42 lines read from src/main.rs\nfn main() {}".to_string(),
        };

        assert_eq!(
            timeline_label_for_invocation(&invocation),
            "read_file · src/main.rs · completed"
        );
    }

    #[test]
    fn timeline_label_compacts_multiline_failures_without_newlines() {
        let invocation = ToolInvocationSummary {
            step_id: 2,
            name: "write_file".to_string(),
            outcome: "error: permission denied while writing Cargo.toml\nretry with approval"
                .to_string(),
        };

        let label = timeline_label_for_invocation(&invocation);
        assert!(!label.contains('\n'));
        assert_eq!(label, "write_file · Cargo.toml · failed");
    }
}
