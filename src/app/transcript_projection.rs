use crate::edit_diff::DEFAULT_EDIT_DIFF_CONTEXT_LINES;
use crate::runtime::task_document::{
    ActiveTurnDocument, AssistantPhase, NoticeSeverity, PulseEntry, TaskDocument,
};
use crate::state::ToolStatus;
use crate::status_contract::{
    WAITING_FOR_RESPONSE_LINE, completed_status_label, pending_status_label,
};
use crate::tool_preview::{ToolPreviewStyle, preview_tool_input};

use super::{StepLifecycle, TranscriptRow};

pub(super) fn project_transcript_rows(
    task_doc: &TaskDocument,
    pre_session_notices: &[String],
) -> Vec<TranscriptRow> {
    let mut rows = project_committed_transcript_rows(task_doc, pre_session_notices);
    rows.extend(project_active_transcript_rows(
        task_doc.active_pulse.as_ref(),
    ));
    rows
}

pub(crate) fn project_committed_transcript_rows(
    task_doc: &TaskDocument,
    pre_session_notices: &[String],
) -> Vec<TranscriptRow> {
    let mut rows: Vec<TranscriptRow> = Vec::new();

    for notice in pre_session_notices {
        rows.push(TranscriptRow::Plain(notice.clone()));
    }

    let mut compaction_iter = task_doc.context_compaction.iter().peekable();

    for (turn_idx, completed) in task_doc.completed_turns.iter().enumerate() {
        while compaction_iter
            .peek()
            .is_some_and(|r| r.turn_index <= turn_idx)
        {
            let record = compaction_iter.next().unwrap();
            rows.push(TranscriptRow::Plain(format!(
                "[context compacted: {}]",
                record.summary
            )));
        }
        append_turn_rows(&mut rows, &completed.entries);
    }

    for record in compaction_iter {
        rows.push(TranscriptRow::Plain(format!(
            "[context compacted: {}]",
            record.summary
        )));
    }

    rows
}

pub(crate) fn project_active_transcript_rows(
    active_pulse: Option<&ActiveTurnDocument>,
) -> Vec<TranscriptRow> {
    let mut rows = Vec::new();
    if let Some(active) = active_pulse {
        append_active_turn_rows(&mut rows, active);
    }
    rows
}

fn append_turn_rows(rows: &mut Vec<TranscriptRow>, entries: &[PulseEntry]) {
    let tool_results: std::collections::HashMap<&str, (&str, bool)> = entries
        .iter()
        .filter_map(|e| {
            if let PulseEntry::ToolResult {
                tool_call_id,
                output,
                is_error,
                ..
            } = e
            {
                Some((tool_call_id.as_str(), (output.as_str(), *is_error)))
            } else {
                None
            }
        })
        .collect();

    let mut last_tool_header: Option<String> = None;
    let mut repeat_count: usize = 0;

    let mut idx = 0;
    while idx < entries.len() {
        match &entries[idx] {
            PulseEntry::UserInput { text, .. } => {
                if !text.trim().is_empty() {
                    rows.push(TranscriptRow::UserInput(text.clone()));
                }
                last_tool_header = None;
                repeat_count = 0;
                idx += 1;
            }
            PulseEntry::AssistantBlock { block, .. } => {
                if block.phase == AssistantPhase::Thinking && block.collapsed {
                    idx += 1;
                    continue;
                }
                let row_before = rows.len();
                for line in block.content.lines() {
                    rows.push(TranscriptRow::AssistantText {
                        text: line.to_string(),
                        streaming: false,
                    });
                }
                if block.streaming
                    && !block.content.is_empty()
                    && !block.content.ends_with('\n')
                    && let Some(last) = rows.get_mut(row_before..)
                    && let Some(TranscriptRow::AssistantText { text, streaming }) = last.last_mut()
                {
                    text.push('▌');
                    *streaming = true;
                }

                if !block.content.trim().is_empty() {
                    last_tool_header = None;
                    repeat_count = 0;
                }
                idx += 1;
            }
            PulseEntry::ToolCall {
                id: call_id,
                name,
                input,
                status,
                ..
            } => {
                let result = tool_results.get(call_id.as_str()).copied();
                match result {
                    Some((output, is_error)) => {
                        let input_preview_for_header = preview_tool_input(
                            name,
                            input,
                            ToolPreviewStyle::Structured,
                            crate::edit_diff::DEFAULT_EDIT_DIFF_CONTEXT_LINES,
                        );
                        let first_output = first_nonempty_line(output).unwrap_or(if is_error {
                            "failed"
                        } else {
                            completed_status_label()
                        });
                        let target = tool_target_summary(first_output)
                            .or_else(|| tool_target_summary(&input_preview_for_header));
                        let current_header = tool_header_summary(
                            name,
                            target,
                            if is_error {
                                "failed"
                            } else {
                                completed_status_label()
                            },
                        );

                        if last_tool_header.as_deref() == Some(&current_header) {
                            repeat_count += 1;
                            let fold_line =
                                format!("(repeated \u{d7}{}, same call)", repeat_count + 1);

                            let replace_last = rows.last().is_some_and(|r| {
                                if let TranscriptRow::ToolDetail(s) = r {
                                    s.starts_with("(repeated")
                                } else {
                                    false
                                }
                            });
                            if replace_last {
                                *rows.last_mut().unwrap() = TranscriptRow::ToolDetail(fold_line);
                            } else {
                                rows.push(TranscriptRow::ToolDetail(fold_line));
                            }
                        } else {
                            repeat_count = 0;
                            last_tool_header = Some(current_header);
                            rows.extend(completed_tool_paragraph_rows(
                                name, input, output, is_error,
                            ));
                        }
                    }
                    None => {
                        let lifecycle = tool_status_to_pending_lifecycle(status);
                        let input_preview = preview_tool_input(
                            name,
                            input,
                            ToolPreviewStyle::Structured,
                            crate::edit_diff::DEFAULT_EDIT_DIFF_CONTEXT_LINES,
                        );
                        let target = tool_target_summary(&input_preview);
                        let pending_header =
                            tool_header_summary(name, target, pending_status_label());
                        if last_tool_header.as_deref() == Some(&pending_header) {
                            repeat_count += 1;
                            let fold_line =
                                format!("(repeated \u{d7}{}, same call)", repeat_count + 1);
                            let replace_last = rows.last().is_some_and(|r| {
                                if let TranscriptRow::ToolDetail(s) = r {
                                    s.starts_with("(repeated")
                                } else {
                                    false
                                }
                            });
                            if replace_last {
                                *rows.last_mut().unwrap() = TranscriptRow::ToolDetail(fold_line);
                            } else {
                                rows.push(TranscriptRow::ToolDetail(fold_line));
                            }
                        } else {
                            repeat_count = 0;
                            last_tool_header = Some(pending_header);
                            rows.extend(pending_tool_paragraph_rows(
                                name,
                                &input_preview,
                                lifecycle,
                            ));
                        }
                    }
                }
                idx += 1;
            }
            PulseEntry::ToolResult { .. } => {
                idx += 1;
            }
            PulseEntry::SystemNotice {
                message, severity, ..
            } => {
                match severity {
                    NoticeSeverity::Error => rows.push(TranscriptRow::Error(message.clone())),
                    _ => rows.push(TranscriptRow::Plain(message.clone())),
                }
                last_tool_header = None;
                repeat_count = 0;
                idx += 1;
            }
            PulseEntry::ApprovalRequest { .. }
            | PulseEntry::ApprovalResolved { .. }
            | PulseEntry::CommandSession { .. } => {
                idx += 1;
            }
        }
    }
}

fn append_active_turn_rows(rows: &mut Vec<TranscriptRow>, active: &ActiveTurnDocument) {
    append_turn_rows(rows, &active.entries);

    let has_streamed_content = active.entries.iter().any(|e| {
        matches!(
            e,
            PulseEntry::AssistantBlock { .. }
                | PulseEntry::ToolCall { .. }
                | PulseEntry::ToolResult { .. }
        )
    });

    if !has_streamed_content {
        let mut line = WAITING_FOR_RESPONSE_LINE.to_string();
        if let Some(ref progress) = active.prompt_progress
            && let (Some(processed), Some(total)) = (progress.processed, progress.total)
        {
            line.push_str(&format!(" \u{2191}:{processed}/{total}"));
        }
        rows.push(TranscriptRow::WaitingPlaceholder(line));
    }
}

fn tool_status_to_pending_lifecycle(status: &ToolStatus) -> StepLifecycle {
    match status {
        ToolStatus::WaitingApproval => StepLifecycle::AwaitingApproval,
        ToolStatus::Executing => StepLifecycle::Running,
        _ => StepLifecycle::Running,
    }
}

fn pending_tool_paragraph_rows(
    name: &str,
    input_preview: &str,
    lifecycle: StepLifecycle,
) -> Vec<TranscriptRow> {
    let status = match lifecycle {
        StepLifecycle::AwaitingApproval => "awaiting approval",
        StepLifecycle::Approved => "approved",
        _ => pending_status_label(),
    };
    let target = tool_target_summary(input_preview);
    let mut rows = vec![TranscriptRow::ToolHeader(tool_header_summary(
        name, target, status,
    ))];
    append_preview_rows(&mut rows, preview_rows(input_preview));
    rows
}

fn completed_tool_paragraph_rows(
    name: &str,
    input: &serde_json::Value,
    output: &str,
    is_error: bool,
) -> Vec<TranscriptRow> {
    let status = if is_error {
        "failed"
    } else {
        completed_status_label()
    };
    let input_preview = preview_tool_input(
        name,
        input,
        ToolPreviewStyle::Structured,
        DEFAULT_EDIT_DIFF_CONTEXT_LINES,
    );
    let first_line = first_nonempty_line(output).unwrap_or(status);
    let target = tool_target_summary(first_line).or_else(|| tool_target_summary(&input_preview));
    let mut rows = vec![TranscriptRow::ToolHeader(tool_header_summary(
        name, target, status,
    ))];
    let input_rows = preview_rows(&input_preview);
    if !input_rows.is_empty()
        && input_rows
            .first()
            .is_some_and(|line| line != "(no arguments)")
    {
        append_preview_rows(&mut rows, input_rows);
    }
    rows.push(TranscriptRow::ToolDetail(format!(
        "Result: {}",
        compact_outcome_summary(first_line)
    )));
    const MAX_EVIDENCE_LINES: usize = 3;
    let nonempty: Vec<&str> = output
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect();
    for line in nonempty.iter().take(MAX_EVIDENCE_LINES) {
        rows.push(TranscriptRow::Evidence((*line).to_string()));
    }
    if nonempty.len() > MAX_EVIDENCE_LINES {
        rows.push(TranscriptRow::Evidence(format!(
            "+{} more lines",
            nonempty.len() - MAX_EVIDENCE_LINES
        )));
    }
    rows
}

fn first_nonempty_line(text: &str) -> Option<&str> {
    text.lines()
        .map(str::trim_end)
        .find(|line| !line.trim().is_empty())
}

fn preview_rows(preview: &str) -> Vec<String> {
    preview
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn append_preview_rows(rows: &mut Vec<TranscriptRow>, preview_rows: Vec<String>) {
    let mut preview_rows = preview_rows.into_iter();
    let Some(first) = preview_rows.next() else {
        return;
    };
    rows.push(TranscriptRow::ToolDetail(format!("Input: {first}")));
    for line in preview_rows {
        rows.push(TranscriptRow::Evidence(line));
    }
}

fn tool_header_summary(name: &str, target: Option<String>, status: &str) -> String {
    match target {
        Some(target) if !target.is_empty() => format!("{name} · {target} · {status}"),
        _ => format!("{name} · {status}"),
    }
}

fn compact_outcome_summary(line: &str) -> String {
    const MAX_SUMMARY_WIDTH: usize = 60;
    let trimmed = line.trim();
    if trimmed.len() <= MAX_SUMMARY_WIDTH {
        return trimmed.to_string();
    }
    let mut end = trimmed.floor_char_boundary(MAX_SUMMARY_WIDTH);
    if let Some(space_pos) = trimmed[..end].rfind(' ')
        && space_pos > MAX_SUMMARY_WIDTH / 2
    {
        end = space_pos;
    }
    format!("{}\u{2026}", &trimmed[..end])
}

fn tool_target_summary(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lowered = trimmed.to_ascii_lowercase();
    for marker in [" from ", " to ", " in ", " at ", " into ", " on "] {
        if let Some(index) = lowered.find(marker)
            && let Some(candidate) = first_pathish_token(&trimmed[index + marker.len()..])
        {
            return Some(candidate);
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

pub(super) fn extract_assistant_response(entries: &[PulseEntry]) -> String {
    let mut parts = Vec::new();
    for entry in entries {
        if let PulseEntry::AssistantBlock { block, .. } = entry
            && block.phase == AssistantPhase::Final
        {
            parts.push(block.content.as_str());
        }
    }
    parts.join("\n")
}
