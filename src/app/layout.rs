use super::*;
use crate::status_contract::{pending_status_label, waiting_for_response_line};

mod helpers;

use self::helpers::display_status_text;
#[cfg(test)]
use self::helpers::{
    compact_outcome_summary, extend_visual_rows, timeline_label_for_invocation,
    tool_outcome_is_error, tool_scope_detail, tool_target_summary,
};

fn telemetry_waiting_summary(rows: &[TranscriptRow]) -> Option<String> {
    rows.iter().rev().find_map(|row| {
        if let TranscriptRow::WaitingPlaceholder(line) = row {
            line.strip_prefix(waiting_for_response_line())
                .map(str::trim)
                .filter(|suffix| !suffix.is_empty())
                .map(ToOwned::to_owned)
        } else {
            None
        }
    })
}

fn telemetry_timing_summary(rows: &[TranscriptRow]) -> Option<String> {
    rows.iter().rev().find_map(|row| {
        if let TranscriptRow::Plain(line) = row {
            (line.starts_with('[') && line.ends_with(']') && line.contains("total:")).then(|| {
                line.trim_start_matches('[')
                    .trim_end_matches(']')
                    .to_string()
            })
        } else {
            None
        }
    })
}

fn telemetry_context_summary(
    context: Option<&AssembledContext>,
) -> Option<TaskContextSummaryState> {
    context.map(|context| TaskContextSummaryState {
        file_rollups: context.file_rollups.len(),
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
        use crate::runtime::task_document::PulseEntry;
        let mut entries = Vec::new();

        let (input, turn_entries): (&str, &[PulseEntry]) =
            if let Some(active) = self.task_doc.active_pulse.as_ref() {
                (active.input.as_str(), &active.entries)
            } else if let Some(last) = self.task_doc.completed_turns.last() {
                (last.input.as_str(), &last.entries)
            } else {
                return entries;
            };

        if !input.trim().is_empty() {
            entries.push(TaskStepView {
                step_id: 0,
                lifecycle: StepLifecycle::UserInput,
                label: input.lines().next().unwrap_or("").to_string(),
                detail: input.to_string(),
                session_id: None,
            });
        }

        let awaiting_step_id = self
            .overlay_state
            .pending_approval
            .as_ref()
            .and_then(|pending| pending.step_id);
        for entry in turn_entries {
            match entry {
                PulseEntry::ToolCall {
                    step_id,
                    name,
                    input,
                    status,
                    ..
                } => {
                    use crate::state::ToolStatus;
                    let lifecycle = if awaiting_step_id == Some(*step_id) {
                        StepLifecycle::AwaitingApproval
                    } else if self.overlay_state.approved_tool_steps.contains(step_id) {
                        StepLifecycle::Approved
                    } else {
                        match status {
                            ToolStatus::Complete => StepLifecycle::Completed,
                            ToolStatus::Error => StepLifecycle::Failed,
                            ToolStatus::Pending
                            | ToolStatus::WaitingApproval
                            | ToolStatus::Executing => StepLifecycle::Running,
                            ToolStatus::Cancelled => StepLifecycle::Failed,
                        }
                    };
                    let input_preview =
                        serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string());
                    let status_label = match lifecycle {
                        StepLifecycle::AwaitingApproval => "awaiting approval",
                        StepLifecycle::Approved => "approved",
                        StepLifecycle::Completed => "done",
                        StepLifecycle::Failed => "failed",
                        _ => pending_status_label(),
                    };
                    entries.push(TaskStepView {
                        step_id: *step_id,
                        lifecycle,
                        label: format!("{name}: {status_label}"),
                        detail: format!("Tool: {name}\nInput:\n{input_preview}"),
                        session_id: None,
                    });
                }
                PulseEntry::CommandSession { step_id, session } => {
                    let pid = session
                        .pid
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "pending".to_string());
                    let display_status = display_status_text(&session.status);
                    entries.push(TaskStepView {
                        step_id: *step_id,
                        lifecycle: StepLifecycle::CommandSession,
                        label: format!("{}: {}", session.command, display_status),
                        detail: format!(
                            "command: {}\npid: {}\nstatus: {}",
                            session.command, pid, display_status,
                        ),
                        session_id: Some(*step_id),
                    });
                }
                _ => {}
            }
        }

        if let Some(active) = self.task_doc.active_pulse.as_ref() {
            for session in active.command_sessions.values() {
                let display_status = display_status_text(&session.status);
                let pid_text = session
                    .pid
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "pending".to_string());
                entries.push(TaskStepView {
                    step_id: session.session_id,
                    lifecycle: StepLifecycle::CommandSession,
                    label: format!("{}: {display_status}", session.command),
                    detail: format!(
                        "command: {}\npid: {}\nstatus: {}",
                        session.command, pid_text, display_status,
                    ),
                    session_id: Some(session.session_id),
                });
            }
        }

        entries
    }

    pub(super) fn task_output_view(&self) -> (String, Vec<TranscriptRow>, OutputScrollAnchor) {
        let steps = self.task_step_views();
        let entries = Self::task_timeline_entries_from(&steps);
        self.task_output_view_with(&entries)
    }

    fn task_output_view_with(
        &self,
        entries: &[TimelineEntry],
    ) -> (String, Vec<TranscriptRow>, OutputScrollAnchor) {
        self.output_view_for_surface(entries, self.transcript_display_rows())
    }

    fn output_view_for_surface(
        &self,
        entries: &[TimelineEntry],
        transcript_rows: Vec<TranscriptRow>,
    ) -> (String, Vec<TranscriptRow>, OutputScrollAnchor) {
        if !self.timeline_follow_mode && !entries.is_empty() {
            let idx = self
                .selected_timeline_index
                .min(entries.len().saturating_sub(1));
            if let Some(entry) = entries.get(idx) {
                let is_tool_step = !matches!(entry.lifecycle, StepLifecycle::UserInput);
                if is_tool_step && !entry.detail.is_empty() {
                    let rows: Vec<TranscriptRow> = entry
                        .detail
                        .lines()
                        .map(|l| TranscriptRow::Plain(l.to_owned()))
                        .collect();
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

        if !transcript_rows.is_empty() {
            return (
                "Transcript".to_string(),
                transcript_rows,
                OutputScrollAnchor::Bottom,
            );
        }

        (
            "Transcript".to_string(),
            Vec::new(),
            OutputScrollAnchor::Bottom,
        )
    }

    fn transcript_display_rows(&self) -> Vec<TranscriptRow> {
        crate::app::transcript_projection::project_transcript_rows(
            &self.task_doc,
            &self.pre_session_notices,
        )
    }

    fn visible_changed_files(&self) -> Vec<String> {
        let mut visible = std::collections::BTreeSet::new();
        for pulse in &self.task_doc.completed_turns {
            visible.extend(pulse.changed_files.iter().cloned());
        }
        if let Some(active) = self.task_doc.active_pulse.as_ref() {
            visible.extend(active.changed_files.iter().cloned());
        }
        visible.into_iter().collect()
    }

    fn build_task_layout_state_with_output_view(
        &self,
        output_view: impl FnOnce(
            &Self,
            &[TimelineEntry],
        ) -> (String, Vec<TranscriptRow>, OutputScrollAnchor),
    ) -> Option<TaskLayoutState> {
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

        let selected_step = if self.timeline_follow_mode && total_steps > 0 {
            total_steps - 1
        } else {
            self.selected_timeline_index
                .min(total_steps.saturating_sub(1))
        };
        let (output_title, output_rows, output_scroll_anchor) =
            output_view(self, &timeline_entries);
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

        Some(TaskLayoutState {
            task_id: self.task_doc.info.id.clone(),
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
            composer_text: String::new(),
            composer_cursor: 0,
            composer_focused: self.composer_is_focused(),
            changed_files,
            follow_mode: self.timeline_follow_mode,
            picker_overlay: vec![],
            working_dir: self.working_dir.display().to_string(),
            model_url: self.model_url.clone(),
        })
    }

    pub fn task_layout_state(&self) -> Option<TaskLayoutState> {
        self.build_task_layout_state_with_output_view(|mode, timeline_entries| {
            mode.task_output_view_with(timeline_entries)
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
        TuiMode, compact_outcome_summary, timeline_label_for_invocation, tool_outcome_is_error,
        tool_scope_detail, tool_target_summary,
    };
    use crate::app::ToolInvocationSummary;

    #[test]
    fn short_outcome_preserved() {
        assert_eq!(compact_outcome_summary("ok"), "ok");
        assert_eq!(compact_outcome_summary("42 lines"), "42 lines");
    }

    #[test]
    fn long_outcome_shortened_at_word_boundary() {
        let long = "this is a long outcome line that exceeds sixty characters and should be shortened at a word boundary";
        let result = compact_outcome_summary(long);
        assert!(
            result.len() <= 65,
            "shortened result must be compact: got {result:?}"
        );
        assert!(
            result.ends_with('\u{2026}'),
            "shortened result must end with ellipsis: got {result:?}"
        );
        assert!(
            result.contains("this is a long"),
            "shortened result must preserve the start: got {result:?}"
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
            "Tool invocation recorded in the completed pulse."
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
    fn transcript_display_rows_projects_from_task_doc() {
        use crate::runtime::task_document::TaskDocumentCondenser;
        let mut mode = TuiMode::new();
        let condenser = TaskDocumentCondenser;
        condenser.begin_turn(
            &mut mode.task_doc,
            "> hi".to_string(),
            0,
            Default::default(),
        );

        mode.push_document_notice(
            "Hello! How can I help you today?".to_string(),
            crate::runtime::NoticeSeverity::Info,
        );

        let rows = mode.transcript_display_rows();
        assert!(
            rows.iter().any(|r| r.as_display_str().contains("Hello!")),
            "transcript rows should include notice: {rows:?}"
        );
    }

    #[test]
    fn transcript_display_rows_empty_when_no_turn_data() {
        let mode = TuiMode::new();
        let rows = mode.transcript_display_rows();
        assert!(rows.is_empty(), "expected empty transcript: {rows:?}");
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

        let blank_count = rows.iter().filter(|r| r.is_empty()).count();
        assert!(
            blank_count <= 2,
            "expected at most 2 blank rows, got {blank_count}: {rows:?}"
        );
        assert_eq!(rows.first().unwrap(), "line1");
        assert_eq!(rows.last().unwrap(), "line2");
    }
}
