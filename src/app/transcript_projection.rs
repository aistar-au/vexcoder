/// Project a flat, ordered list of transcript display rows from a `TaskDocument`.
///
/// This is the single source of truth for all transcript rendering: layout,
/// scroll helpers, and tests all call this function rather than reading from a
/// separate `history_state.lines` buffer.
use crate::runtime::task_document::{
    ActiveTurnDocument, AssistantPhase, NoticeSeverity, TaskDocument, TurnEntry,
};
use crate::state::ToolStatus;
use crate::status_contract::WAITING_FOR_RESPONSE_LINE;
use crate::tool_preview::{preview_tool_input, ToolPreviewStyle};

use super::layout::{completed_tool_paragraph_rows, pending_tool_paragraph_rows};
use super::StepLifecycle;

/// Build the full ordered list of transcript rows from `task_doc`.
///
/// `pre_session_notices` are system messages that arrived before the first
/// turn (e.g. notes warnings, sandbox state).  They appear at the top.
pub(super) fn project_transcript_rows(
    task_doc: &TaskDocument,
    pre_session_notices: &[String],
) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();

    for notice in pre_session_notices {
        rows.push(notice.clone());
    }

    for completed in &task_doc.completed_turns {
        append_turn_rows(&mut rows, &completed.entries);
    }

    if let Some(active) = &task_doc.active_turn {
        append_active_turn_rows(&mut rows, active);
    }

    rows
}

fn append_turn_rows(rows: &mut Vec<String>, entries: &[TurnEntry]) {
    // Pre-index ToolResult entries by tool_call_id for O(1) lookup while
    // projecting ToolCall entries, avoiding an O(n²) scan per entry.
    let tool_results: std::collections::HashMap<&str, (&str, bool)> = entries
        .iter()
        .filter_map(|e| {
            if let TurnEntry::ToolResult {
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

    let mut idx = 0;
    while idx < entries.len() {
        match &entries[idx] {
            TurnEntry::UserInput { text, .. } => {
                if !text.trim().is_empty() {
                    rows.push(format!("> {text}"));
                }
                idx += 1;
            }
            TurnEntry::AssistantBlock { block, .. } => {
                if block.phase == AssistantPhase::Thinking && block.collapsed {
                    idx += 1;
                    continue;
                }
                let row_before = rows.len();
                for line in block.content.lines() {
                    rows.push(line.to_string());
                }
                if block.streaming && !block.content.is_empty() && !block.content.ends_with('\n') {
                    if let Some(last) = rows.get_mut(row_before..).and_then(|s| s.last_mut()) {
                        last.push('▌');
                    }
                }
                idx += 1;
            }
            TurnEntry::ToolCall {
                id: call_id,
                name,
                input,
                status,
                ..
            } => {
                let result = tool_results.get(call_id.as_str()).copied();
                match result {
                    Some((output, is_error)) => {
                        rows.extend(completed_tool_paragraph_rows(name, input, output, is_error));
                    }
                    None => {
                        let lifecycle = tool_status_to_pending_lifecycle(status);
                        let input_preview = preview_tool_input(
                            name,
                            input,
                            ToolPreviewStyle::Structured,
                            crate::edit_diff::DEFAULT_EDIT_DIFF_CONTEXT_LINES,
                        );
                        rows.extend(pending_tool_paragraph_rows(name, &input_preview, lifecycle));
                    }
                }
                idx += 1;
            }
            TurnEntry::ToolResult { .. } => {
                // Rendered as part of the paired ToolCall entry above.
                idx += 1;
            }
            TurnEntry::SystemNotice {
                message, severity, ..
            } => {
                match severity {
                    NoticeSeverity::Error => rows.push(format!("[error] {message}")),
                    _ => rows.push(message.clone()),
                }
                idx += 1;
            }
            TurnEntry::ApprovalRequest { .. }
            | TurnEntry::ApprovalResolved { .. }
            | TurnEntry::CommandSession { .. } => {
                idx += 1;
            }
        }
    }
}

fn append_active_turn_rows(rows: &mut Vec<String>, active: &ActiveTurnDocument) {
    append_turn_rows(rows, &active.entries);

    // If no streaming content arrived yet, show a waiting placeholder.
    let has_streamed_content = active.entries.iter().any(|e| {
        matches!(
            e,
            TurnEntry::AssistantBlock { .. }
                | TurnEntry::ToolCall { .. }
                | TurnEntry::ToolResult { .. }
        )
    });

    if !has_streamed_content {
        let mut line = WAITING_FOR_RESPONSE_LINE.to_string();
        if let Some(ref progress) = active.prompt_progress {
            if let (Some(processed), Some(total)) = (progress.processed, progress.total) {
                line.push_str(&format!(" \u{2191}:{processed}/{total}"));
            }
        }
        rows.push(line);
    }
}

fn tool_status_to_pending_lifecycle(status: &ToolStatus) -> StepLifecycle {
    match status {
        ToolStatus::WaitingApproval => StepLifecycle::AwaitingApproval,
        ToolStatus::Executing => StepLifecycle::Running,
        _ => StepLifecycle::Running,
    }
}

/// Extract the concatenated `FinalText` assistant response from a completed
/// turn's entry list (used for auto-memory extraction).
pub(super) fn extract_assistant_response(entries: &[TurnEntry]) -> String {
    let mut parts = Vec::new();
    for entry in entries {
        if let TurnEntry::AssistantBlock { block, .. } = entry {
            if block.phase == AssistantPhase::Final {
                parts.push(block.content.as_str());
            }
        }
    }
    parts.join("\n")
}
