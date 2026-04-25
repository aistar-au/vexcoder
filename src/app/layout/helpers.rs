#[cfg(test)]
use super::ToolInvocationSummary;
use crate::status_contract::{completed_status_label, pending_status_label};

#[cfg(test)]
use crate::api::client::builtin_tool_summaries;

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
pub(super) fn compact_outcome_summary(line: &str) -> String {
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

#[cfg(test)]
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
        .unwrap_or_else(|| "Tool invocation recorded in the completed pulse.".to_string())
}

#[cfg(test)]
pub(super) fn tool_target_summary(line: &str) -> Option<String> {
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

#[cfg(test)]
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
