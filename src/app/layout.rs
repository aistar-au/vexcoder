use super::*;
use crate::status_contract::{
    is_waiting_placeholder, pending_status_label, waiting_for_response_line,
};

mod helpers;

#[cfg(test)]
use self::helpers::{compact_outcome_summary, tool_scope_detail, tool_target_summary};
pub(crate) use self::helpers::{
    completed_tool_paragraph_rows, pending_tool_paragraph_rows, tool_approval_paragraph_rows,
};
use self::helpers::{
    display_status_text, extend_visual_rows, timeline_label_for_invocation, tool_outcome_is_error,
};

fn format_waiting_status(
    elapsed: String,
    prompt_progress: Option<&crate::types::StreamPromptProgress>,
) -> String {
    let mut status = format!("{} {elapsed}", waiting_for_response_line());
    if let Some(progress) = prompt_progress {
        if let (Some(processed), Some(total)) = (progress.processed, progress.total) {
            status.push_str(&format!(" | \u{2191}:{processed}/{total}"));
        }
    }
    status
}

fn telemetry_waiting_summary(rows: &[String]) -> Option<String> {
    rows.iter().rev().find_map(|line| {
        line.strip_prefix(waiting_for_response_line())
            .map(str::trim)
            .filter(|suffix| !suffix.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn telemetry_timing_summary(rows: &[String]) -> Option<String> {
    rows.iter().rev().find_map(|line| {
        (line.starts_with('[') && line.ends_with(']') && line.contains("total:")).then(|| {
            line.trim_start_matches('[')
                .trim_end_matches(']')
                .to_string()
        })
    })
}

fn telemetry_context_summary(
    context: Option<&AssembledContext>,
) -> Option<TaskContextSummaryState> {
    context.map(|context| TaskContextSummaryState {
        file_snapshots: context.file_snapshots.len(),
        related_paths: context.related_paths.len(),
        cache_hits: context.cache_hits,
        cache_misses: context.cache_misses,
        git_context_included: context.git_status_summary.is_some() || context.recent_diff.is_some(),
    })
}

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
    /// completed turn so the task surface remains populated.
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
                    _ => pending_status_label(),
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
            let display_status = display_status_text(&session.status);
            entries.push(TaskStepView {
                step_id: session.id,
                lifecycle: StepLifecycle::CommandSession,
                label: format!("{}: {}", session.command, display_status),
                detail: format!(
                    "command: {}\npid: {}\nstatus: {}",
                    session.command, pid, display_status,
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
                    return (
                        format!(
                            "Inspector \u{00b7} {}/{} \u{00b7} {} \u{00b7} {} rows",
                            idx + 1,
                            entries.len(),
                            entry.label,
                            rows.len(),
                        ),
                        rows,
                        OutputScrollAnchor::Top,
                    );
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
        let assistant_index = self.history_state.active_assistant_index;
        let skip_assistant_line =
            if self.history_state.turn_in_progress && !self.history_state.cancel_pending {
                if self.current_turn_response.is_empty() {
                    assistant_index.filter(|idx| {
                        let current_line = self
                            .history_state
                            .lines
                            .get(*idx)
                            .map(String::as_str)
                            .unwrap_or("");
                        (current_line.is_empty() || is_waiting_placeholder(current_line))
                            && self
                                .history_state
                                .lines
                                .iter()
                                .skip(idx.saturating_add(1))
                                .any(|line| !line.is_empty())
                    })
                } else {
                    assistant_index
                }
            } else {
                None
            };

        let mut rows = Vec::new();
        extend_visual_rows(&mut rows, &self.history_state.lines, skip_assistant_line);

        if self.history_state.turn_in_progress && !self.history_state.cancel_pending {
            if self.current_turn_response.is_empty() {
                // No content yet — show the waiting status with elapsed time
                // and prompt-eval progress counters.
                let elapsed = self
                    .turn_started_at
                    .map(|t| format!("{:.1}s", t.elapsed().as_secs_f64()))
                    .unwrap_or_default();
                if rows
                    .last()
                    .is_some_and(|line| line.is_empty() || is_waiting_placeholder(line))
                {
                    if let Some(last) = rows.last_mut() {
                        *last = format_waiting_status(
                            elapsed,
                            self.current_turn_prompt_progress.as_ref(),
                        );
                    }
                }
            } else {
                // Response is streaming — rebuild the visible response from the
                // canonical current_turn_response and append it after any tool
                // or approval transcript rows that were recorded during the
                // turn.
                if rows
                    .last()
                    .is_some_and(|line| line.is_empty() || is_waiting_placeholder(line))
                {
                    rows.pop();
                }
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
        }

        rows
    }

    fn visible_changed_files(&self) -> Vec<String> {
        let mut visible = std::collections::BTreeSet::new();
        visible.extend(
            self.current_task
                .changed_files
                .iter()
                .map(|path| path.to_string_lossy().into_owned()),
        );
        visible.extend(self.current_turn_changed_files.iter().cloned());
        visible.into_iter().collect()
    }

    pub fn task_layout_state(&self) -> Option<TaskLayoutState> {
        // Always return the task-state control surface. The fullscreen CLI/app
        // stays in the top transcript + prompt + status-bar arrangement
        // between tool calls and after turn completion instead of yielding back
        // to a separate transcript-only layout.
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
        // When follow mode is active, snap the selection to the latest entry
        // so the status bar and any inspector switch reflect the current head
        // rather than a stale index from before new entries arrived.
        let selected_step = if self.timeline_follow_mode && total_steps > 0 {
            total_steps - 1
        } else {
            self.selected_timeline_index
                .min(total_steps.saturating_sub(1))
        };
        let (output_title, output_rows, output_scroll_anchor) =
            self.task_output_view_with(&timeline_entries);
        let output_scroll_offset = match output_scroll_anchor {
            OutputScrollAnchor::Bottom => self.transcript_scroll_offset,
            OutputScrollAnchor::Top => self.inspector_scroll_offset,
        };
        let active_tools = timeline_entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry.lifecycle,
                    StepLifecycle::Running
                        | StepLifecycle::AwaitingApproval
                        | StepLifecycle::Approved
                )
            })
            .count();
        let active_commands = timeline_entries
            .iter()
            .filter(|entry| entry.lifecycle == StepLifecycle::CommandSession)
            .count();
        let changed_files = self.visible_changed_files();

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
            telemetry: TaskTelemetryState {
                mode: self.mode_status_label().to_string(),
                approval: self.approval_status_label().to_string(),
                model_name: self.model_name.clone(),
                model_backend: Some(self.model_backend),
                sandbox_kind: Some(self.sandbox.kind()),
                context_summary: telemetry_context_summary(self.last_assembled_context.as_ref()),
                history_rows: self.history_row_count(),
                total_tokens: self.total_session_tokens(),
                tokens_sent: self.tokens_sent_total(),
                tokens_received: self.tokens_received_total(),
                active_tools,
                active_commands,
                waiting_summary: telemetry_waiting_summary(&output_rows),
                timing_summary: telemetry_timing_summary(&output_rows),
                git_branch: self.git_branch.clone(),
            },
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
            changed_files,
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

#[cfg(test)]
mod tests {
    use super::{
        compact_outcome_summary, timeline_label_for_invocation, tool_outcome_is_error,
        tool_scope_detail, tool_target_summary, waiting_for_response_line, HistoryState, TuiMode,
    };
    use crate::app::ToolInvocationSummary;
    use crate::types::StreamPromptProgress;

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
            "read_file · src/main.rs · Response complete."
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

    #[test]
    fn transcript_display_rows_replaces_streaming_history_tail_with_current_response() {
        let mut mode = TuiMode::new();
        mode.history_state = HistoryState {
            lines: vec!["> hi".to_string(), "Hello".to_string()],
            turn_in_progress: true,
            cancel_pending: false,
            active_assistant_index: Some(1),
            scroll_offset: 0,
            auto_follow: true,
        };
        mode.current_turn_response = "Hello!\nHow can I help you today?".to_string();

        assert_eq!(
            mode.transcript_display_rows(),
            vec![
                "> hi".to_string(),
                "Hello!".to_string(),
                "How can I help you today?▌".to_string(),
            ]
        );
    }

    #[test]
    fn transcript_display_rows_keeps_formatted_waiting_status_inline() {
        let mut mode = TuiMode::new();
        mode.history_state = HistoryState {
            lines: vec!["> hi".to_string(), waiting_for_response_line().to_string()],
            turn_in_progress: true,
            cancel_pending: false,
            active_assistant_index: Some(1),
            scroll_offset: 0,
            auto_follow: true,
        };
        mode.current_turn_prompt_progress = Some(StreamPromptProgress {
            total: Some(2641),
            processed: Some(512),
            cache: None,
            time_ms: None,
        });

        let rows = mode.transcript_display_rows();
        assert_eq!(rows[0], "> hi");
        assert!(rows[1].starts_with(waiting_for_response_line()));
        assert!(rows[1].contains("\u{2191}:512/2641"));
    }

    #[test]
    fn completed_tool_paragraphs_truncate_at_six_lines_with_overflow() {
        use super::completed_tool_paragraph_rows;

        let output_lines: Vec<String> = (1..=20).map(|i| format!("output line {i}")).collect();
        let output = output_lines.join("\n");

        let rows = completed_tool_paragraph_rows(
            "bash",
            &serde_json::json!({"command": "ls -la"}),
            &output,
            false,
        );

        let evidence_rows: Vec<&String> = rows
            .iter()
            .filter(|r| r.starts_with("[evidence]"))
            .collect();

        assert_eq!(
            evidence_rows.len(),
            4,
            "should have 3 evidence lines + 1 overflow indicator: {evidence_rows:?}"
        );
        assert!(
            evidence_rows.last().unwrap().contains("+17 more lines"),
            "last evidence row should indicate remaining lines: {:?}",
            evidence_rows.last()
        );
    }

    #[test]
    fn consecutive_blank_lines_collapsed_in_visual_rows() {
        use super::extend_visual_rows;
        let lines: Vec<String> = vec![
            "line1".to_string(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            "line2".to_string(),
        ];
        let mut rows = Vec::new();
        extend_visual_rows(&mut rows, &lines, None);
        // Should have line1, at most 2 blanks, then line2.
        let blank_count = rows.iter().filter(|r| r.is_empty()).count();
        assert!(
            blank_count <= 2,
            "expected at most 2 blank rows, got {blank_count}: {rows:?}"
        );
        assert_eq!(rows.first().unwrap(), "line1");
        assert_eq!(rows.last().unwrap(), "line2");
    }
}
