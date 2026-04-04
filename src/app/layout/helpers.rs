use super::{PendingTurnToolCall, StepLifecycle, ToolInvocationSummary};
use crate::edit_diff::DEFAULT_EDIT_DIFF_CONTEXT_LINES;
use crate::status_contract::{completed_status_label, pending_status_label};
use crate::tool_preview::{preview_tool_input, ToolPreviewStyle};

#[cfg(test)]
use crate::api::client::builtin_tool_summaries;

pub(super) fn extend_visual_rows(
    rows: &mut Vec<String>,
    history_lines: &[String],
    skip_index: Option<usize>,
) {
    let mut consecutive_blanks: usize = 0;
    for (index, line) in history_lines.iter().enumerate() {
        if skip_index == Some(index) {
            continue;
        }
        if line.is_empty() {
            consecutive_blanks += 1;
            if consecutive_blanks <= 2 {
                rows.push(String::new());
            }
        } else {
            consecutive_blanks = 0;
            rows.extend(line.lines().map(ToOwned::to_owned));
        }
    }
}

pub(super) fn display_status_text(status: &str) -> &str {
    match status {
        "running" => pending_status_label(),
        "completed" => completed_status_label(),
        _ => status,
    }
}

pub(super) fn timeline_label_for_invocation(invocation: &ToolInvocationSummary) -> String {
    let is_error = tool_outcome_is_error(&invocation.outcome);
    let status_label = if is_error {
        "failed"
    } else {
        completed_status_label()
    };
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
    } else if result_summary == status_label || (!is_error && result_summary == "ok") {
        format!("{} · {}", invocation.name, status_label)
    } else {
        format!(
            "{} · {} · {}",
            invocation.name, result_summary, status_label
        )
    }
}

pub(super) fn compact_outcome_summary(line: &str) -> String {
    const MAX_SUMMARY_WIDTH: usize = 60;
    let trimmed = line.trim();
    if trimmed.len() <= MAX_SUMMARY_WIDTH {
        return trimmed.to_string();
    }
    let mut end = trimmed.floor_char_boundary(MAX_SUMMARY_WIDTH);
    if let Some(space_pos) = trimmed[..end].rfind(' ') {
        if space_pos > MAX_SUMMARY_WIDTH / 2 {
            end = space_pos;
        }
    }
    format!("{}\u{2026}", &trimmed[..end])
}

pub(super) fn tool_outcome_is_error(outcome: &str) -> bool {
    let lowered = outcome.trim().to_ascii_lowercase();
    lowered.starts_with("error")
        || lowered.starts_with("failed")
        || lowered.contains("denied")
        || lowered.starts_with("cancelled")
        || lowered.starts_with("canceled")
}

#[cfg(test)]
pub(super) fn tool_scope_detail(tool_name: &str) -> String {
    builtin_tool_summaries()
        .into_iter()
        .find(|tool| tool.name == tool_name)
        .map(|tool| tool.description)
        .unwrap_or_else(|| "Tool invocation recorded in the completed turn.".to_string())
}

pub(super) fn tool_target_summary(line: &str) -> Option<String> {
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

fn first_nonempty_line(text: &str) -> Option<&str> {
    text.lines()
        .map(str::trim_end)
        .find(|line| !line.trim().is_empty())
}

fn compact_preview_text(text: &str) -> String {
    let compact = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if compact.is_empty() {
        compact
    } else {
        compact_outcome_summary(&compact)
    }
}

fn preview_rows(preview: &str) -> Vec<String> {
    preview
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn append_preview_rows(rows: &mut Vec<String>, preview_rows: Vec<String>) {
    let mut preview_rows = preview_rows.into_iter();
    let Some(first) = preview_rows.next() else {
        return;
    };

    rows.push(format!("[detail] Input: {first}"));
    for line in preview_rows {
        rows.push(format!("[evidence] {line}"));
    }
}

fn tool_header_summary(name: &str, target: Option<String>, status: &str) -> String {
    match target {
        Some(target) if !target.is_empty() => format!("{name} · {target} · {status}"),
        _ => format!("{name} · {status}"),
    }
}

pub(crate) fn pending_tool_paragraph_rows(
    pending: &PendingTurnToolCall,
    lifecycle: StepLifecycle,
) -> Vec<String> {
    let status = match lifecycle {
        StepLifecycle::AwaitingApproval => "awaiting approval",
        StepLifecycle::Approved => "approved",
        _ => pending_status_label(),
    };
    let target = tool_target_summary(&pending.input_preview);
    let mut rows = vec![format!(
        "[tool] {}",
        tool_header_summary(&pending.name, target, status)
    )];
    append_preview_rows(&mut rows, preview_rows(&pending.input_preview));
    rows
}

pub(crate) fn completed_tool_paragraph_rows(
    name: &str,
    input: &serde_json::Value,
    output: &str,
    is_error: bool,
) -> Vec<String> {
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
    let mut rows = vec![format!(
        "[tool] {}",
        tool_header_summary(name, target, status)
    )];
    let input_rows = preview_rows(&input_preview);
    if !input_rows.is_empty()
        && input_rows
            .first()
            .is_some_and(|line| line != "(no arguments)")
    {
        append_preview_rows(&mut rows, input_rows);
    }
    rows.push(format!(
        "[detail] Result: {}",
        compact_outcome_summary(first_line)
    ));
    const MAX_EVIDENCE_LINES: usize = 3;
    let nonempty: Vec<&str> = output
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect();
    for line in nonempty.iter().take(MAX_EVIDENCE_LINES) {
        rows.push(format!("[evidence] {line}"));
    }
    if nonempty.len() > MAX_EVIDENCE_LINES {
        rows.push(format!(
            "[evidence] +{} more lines",
            nonempty.len() - MAX_EVIDENCE_LINES
        ));
    }
    rows
}

pub(crate) fn tool_approval_paragraph_rows(summary: &str, input_preview: &str) -> Vec<String> {
    let mut rows = vec![format!("[approval] {summary}")];
    let input_detail = compact_preview_text(input_preview);
    if !input_detail.is_empty() {
        rows.push(format!("[approval_detail] Input: {input_detail}"));
    }
    rows
}
