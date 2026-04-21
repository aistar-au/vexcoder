use crate::app::{TaskViewProjection, TranscriptRow};
use crate::status_contract::{StatusTone, pending_status_label, status_tone};
use crate::ui::input_metrics::{char_display_width, display_width, truncate_to_display_width};
use crate::ui::tui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use ansi_to_tui::IntoText;

pub(crate) fn transcript_output_line(row: &TranscriptRow) -> Line<'static> {
    match row {
        TranscriptRow::WaitingPlaceholder(_) => Line::from(vec![
            Span::styled(
                "  ⋯ ",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                pending_status_label().to_string(),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::DIM),
            ),
        ]),
        TranscriptRow::Error(rest) => Line::from(vec![
            Span::styled(
                "  ✖ ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                rest.clone(),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        ]),
        TranscriptRow::ToolHeader(rest) => render_tool_header(rest),
        TranscriptRow::ToolDetail(rest) => structured_transcript_line(rest, "    ", None),
        TranscriptRow::Evidence(rest) => {
            structured_transcript_line(rest, "      ", Some("\u{2727} "))
        }
        TranscriptRow::UserInput(text) => Line::from(vec![
            Span::styled(
                "> ",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            ),
            Span::styled(
                text.clone(),
                Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
            ),
        ]),
        TranscriptRow::AssistantText { text, .. } => render_assistant_text(text),
        TranscriptRow::Plain(s) => render_plain_row(s),
    }
}

/// Render a `ToolHeader` row: `"⬧ name · target · status"`.
fn render_tool_header(rest: &str) -> Line<'static> {
    let mut spans = vec![Span::styled(
        "  \u{2726} ",
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    )];
    if let Some((leading, status)) = split_tool_summary(rest) {
        for (index, segment) in leading.iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled(
                    " \u{00b7} ",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                ));
            }
            spans.push(Span::styled(
                (*segment).to_string(),
                if index == 0 {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::Gray)
                },
            ));
        }
        spans.push(Span::styled(
            " \u{00b7} ",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ));
        spans.push(Span::styled(status.to_string(), tool_status_style(status)));
    } else {
        spans.push(Span::styled(
            rest.to_string(),
            Style::default().fg(Color::White),
        ));
    }
    Line::from(spans)
}

/// Render an `AssistantText` row (ANSI, markdown, or plain).
fn render_assistant_text(text: &str) -> Line<'static> {
    if text.contains('\x1b') {
        match text.into_text() {
            Ok(t) => t.lines.into_iter().next().unwrap_or_else(|| Line::from("")),
            Err(_) => Line::from(Span::styled(
                text.to_string(),
                Style::default().fg(Color::White),
            )),
        }
    } else if looks_like_inline_markdown(text) {
        super::markdown_to_inline_line(text)
            .unwrap_or_else(|| Line::from(Span::raw(text.to_string())))
    } else {
        Line::from(Span::styled(
            text.to_string(),
            Style::default().fg(Color::White),
        ))
    }
}

/// Render a `Plain` row — handles older runtime marker prefixes for command
/// sessions, approval notices, and other free-form system strings.
fn render_plain_row(row: &str) -> Line<'static> {
    if let Some(rest) = row.strip_prefix("[approval] ") {
        Line::from(vec![
            Span::styled(
                "  ? ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    } else if let Some(rest) = row.strip_prefix("[approval_detail] ") {
        Line::styled(
            format!("    {rest}"),
            Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
        )
    } else if let Some((command, pid)) = parse_command_session_started(row) {
        let summary = pid
            .map(|pid| {
                format!(
                    "command session · {command} · pid {pid} · {}",
                    pending_status_label()
                )
            })
            .unwrap_or_else(|| format!("command session · {command} · {}", pending_status_label()));
        render_tool_header(&summary)
    } else if let Some(rest) = row.strip_prefix("[command session exit: ") {
        structured_transcript_line(
            &format!("Exit: {}", rest.trim_end_matches(']')),
            "    ",
            None,
        )
    } else if row == "[command session cancelled]" {
        structured_transcript_line("Status: cancelled", "    ", None)
    } else if row == "[command session cancellation requested]" {
        structured_transcript_line("Status: cancellation requested", "    ", None)
    } else if let Some(rest) = row.strip_prefix("[command session] error: ") {
        Line::from(vec![
            Span::styled(
                "  ✖ ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                rest.to_string(),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        ])
    } else if let Some(rest) = row.strip_prefix("[stderr] ") {
        Line::from(vec![
            Span::styled(
                "      \u{2727} ",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            ),
            Span::styled(
                rest.to_string(),
                Style::default().fg(Color::Red).add_modifier(Modifier::DIM),
            ),
        ])
    } else if row.contains('\x1b') {
        match row.into_text() {
            Ok(text) => text
                .lines
                .into_iter()
                .next()
                .unwrap_or_else(|| Line::from("")),
            Err(_) => Line::from(Span::styled(
                row.to_string(),
                Style::default().fg(Color::White),
            )),
        }
    } else if looks_like_inline_markdown(row) {
        super::markdown_to_inline_line(row)
            .unwrap_or_else(|| Line::from(Span::raw(row.to_string())))
    } else {
        Line::from(Span::styled(
            row.to_string(),
            Style::default().fg(Color::White),
        ))
    }
}

fn looks_like_inline_markdown(row: &str) -> bool {
    !row.contains("```") && (row.starts_with('#') || row.contains("**") || row.contains('`'))
}

pub(crate) fn structured_transcript_line(
    row: &str,
    indent: &'static str,
    marker: Option<&'static str>,
) -> Line<'static> {
    let trimmed = row.trim_start();
    if let Some((color, bold)) = diff_style(trimmed) {
        let mut style = Style::default().fg(color).add_modifier(Modifier::DIM);
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        let mut spans = vec![Span::styled(
            format!("{indent}{}", marker.unwrap_or("")),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        )];
        spans.push(Span::styled(trimmed.to_string(), style));
        return Line::from(spans);
    }
    if looks_like_json_line(trimmed) {
        return json_transcript_line(trimmed, indent, marker);
    }
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        return Line::from(vec![
            Span::styled(
                format!("{indent}{}", marker.unwrap_or("")),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            ),
            Span::styled("• ".to_string(), Style::default().fg(Color::Yellow)),
            Span::styled(
                rest.to_string(),
                Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
            ),
        ]);
    }
    if let Some((prefix, rest, _)) = parse_numbered_list_item(trimmed) {
        return Line::from(vec![
            Span::styled(
                format!("{indent}{}", marker.unwrap_or("")),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            ),
            Span::styled(prefix.to_string(), Style::default().fg(Color::Yellow)),
            Span::styled(
                rest.to_string(),
                Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
            ),
        ]);
    }
    Line::from(vec![
        Span::styled(
            format!("{indent}{}", marker.unwrap_or("")),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ),
        Span::styled(
            trimmed.to_string(),
            Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
        ),
    ])
}

pub(crate) fn json_transcript_line(
    row: &str,
    indent: &'static str,
    marker: Option<&'static str>,
) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{indent}{}", marker.unwrap_or("")),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM),
    )];
    let mut chars = row.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            let mut token = String::from("\"");
            for next in chars.by_ref() {
                token.push(next);
                if next == '"' {
                    break;
                }
            }
            let color = if chars.peek() == Some(&':') {
                Color::Cyan
            } else {
                Color::Green
            };
            spans.push(Span::styled(token, Style::default().fg(color)));
            continue;
        }
        let style = match ch {
            '0'..='9' | '-' => Style::default().fg(Color::Yellow),
            't' | 'f' | 'n' => Style::default().fg(Color::Magenta),
            _ => Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        };
        spans.push(Span::styled(ch.to_string(), style));
    }
    Line::from(spans)
}

pub(crate) fn looks_like_json_line(text: &str) -> bool {
    text.trim_start().starts_with('{')
        || text.trim_start().starts_with('[')
        || text.trim_start().starts_with('}')
        || text.trim_start().starts_with(']')
        || text.contains("\":")
}

pub(crate) fn diff_style(line: &str) -> Option<(Color, bool)> {
    if line.starts_with('+') && !line.starts_with("+++") {
        Some((Color::Green, false))
    } else if line.starts_with('-') && !line.starts_with("---") {
        Some((Color::Red, false))
    } else if line.starts_with("@@")
        || line.starts_with("diff --git")
        || line.starts_with("index ")
        || line.starts_with("+++ ")
        || line.starts_with("--- ")
    {
        Some((Color::Cyan, true))
    } else {
        None
    }
}

pub(crate) fn parse_command_session_started(line: &str) -> Option<(String, Option<String>)> {
    let rest = line.strip_prefix("[command session started")?;
    let (prefix, command) = rest.split_once("] ")?;
    let pid = prefix
        .strip_prefix(" pid=")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    Some((command.to_string(), pid))
}

pub(crate) fn split_tool_summary(text: &str) -> Option<(Vec<&str>, &str)> {
    let segments: Vec<&str> = text.split(" \u{00b7} ").collect();
    let (status, leading) = segments.split_last()?;
    if leading.is_empty() || status_tone(status).is_none() {
        return None;
    }
    Some((leading.to_vec(), *status))
}

pub(crate) fn tool_status_style(status: &str) -> Style {
    Style::default()
        .fg(match status_tone(status) {
            Some(StatusTone::Success) => Color::Green,
            Some(StatusTone::Error) => Color::Red,
            Some(StatusTone::Progress) => Color::Magenta,
            Some(StatusTone::Attention) => Color::Yellow,
            None => Color::White,
        })
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn truncate_line(input: &str, width: usize) -> String {
    let width = width.max(1);
    let mut out = String::new();
    let mut used = 0usize;
    let mut clipped = false;

    for ch in input.chars() {
        let ch_width = char_display_width(ch);
        if used + ch_width > width {
            clipped = true;
            break;
        }
        out.push(ch);
        used += ch_width;
    }

    if clipped && width >= 4 {
        out = truncate_to_display_width(&out, width - 3);
        out.push_str("...");
    }
    out
}

#[cfg(test)]
pub(crate) fn task_output_window(
    state: &TaskViewProjection,
    viewport_width: u16,
    viewport_height: usize,
) -> (usize, usize) {
    let total = expand_rows_for_display(&state.output_rows, viewport_width).len();
    task_output_window_with_total(state, total, viewport_height)
}

/// Compute the visible (start, end) range given a precomputed `total` row
/// count, avoiding a redundant `expand_rows_for_display` call when the
/// caller already has the expanded rows.
pub(crate) fn task_output_window_with_total(
    state: &TaskViewProjection,
    total: usize,
    viewport_height: usize,
) -> (usize, usize) {
    const INSPECTOR_VIEWPORT_ROWS: usize = 6;

    if viewport_height == 0 || total == 0 {
        return (0, 0);
    }

    match state.output_scroll_anchor {
        crate::app::OutputScrollAnchor::Bottom => {
            let max_offset = total.saturating_sub(viewport_height);
            let offset = state.output_scroll_offset.min(max_offset);
            let start = total.saturating_sub(viewport_height.saturating_add(offset));
            let end = (start + viewport_height).min(total);
            (start, end)
        }
        crate::app::OutputScrollAnchor::Top => {
            let inspector_height = viewport_height.clamp(1, INSPECTOR_VIEWPORT_ROWS);
            let start = state.output_scroll_offset.min(total.saturating_sub(1));
            let end = (start + inspector_height).min(total);
            (start, end)
        }
    }
}

pub(crate) fn task_output_render_area(
    _state: &TaskViewProjection,
    area: Rect,
    visible_rows: usize,
) -> Rect {
    if area.height == 0 || visible_rows == 0 {
        return Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 0,
        };
    }

    let height = visible_rows.min(area.height as usize) as u16;
    Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height,
    }
}

pub(crate) fn expand_rows_for_display(rows: &[TranscriptRow], cols: u16) -> Vec<TranscriptRow> {
    if cols < 4 {
        return rows.to_vec();
    }

    let mut expanded = Vec::with_capacity((rows.len() * 3) / 2);
    for row in rows {
        let text = row.as_display_str();
        if text.contains('\n') {
            // UserInput rows may contain expanded [file: path]\n```text\n...\n```
            // blocks from @path mentions submitted to the API.  Collapse those
            // back to compact @path tokens for the transcript display so that
            // attaching a large file does not flood the output pane with
            // thousands of file-content lines.
            let cow: std::borrow::Cow<str> = if matches!(row, TranscriptRow::UserInput(_)) {
                std::borrow::Cow::Owned(collapse_file_blocks_for_display(text))
            } else {
                std::borrow::Cow::Borrowed(text)
            };
            for sub in cow.split('\n') {
                for wrapped in word_wrap_plain_row(sub, cols as usize) {
                    expanded.push(row.clone_with_text(wrapped));
                }
            }
        } else {
            let mut first = true;
            for wrapped in word_wrap_transcript_row(row, cols as usize) {
                if first {
                    expanded.push(row.clone_with_text(wrapped));
                    first = false;
                } else {
                    // Continuation lines of a wrapped row share the same variant.
                    expanded.push(row.clone_with_text(wrapped));
                }
            }
        }
    }
    expanded
}

/// Collapse `[file: path]\n```text\n<content>\n``` ` (and dir equivalents)
/// blocks in a user-input display string back to compact `@path` tokens.
/// The full content is never discarded — it lives in the underlying turn data
/// for the API context.  This only affects what is shown in the TUI pane.
fn collapse_file_blocks_for_display(text: &str) -> String {
    if !text.contains("[file: ") && !text.contains("[dir: ") {
        return text.to_string();
    }
    let mut result: Vec<String> = Vec::new();
    let mut lines = text.lines();
    let mut in_code_fence = false;

    while let Some(line) = lines.next() {
        if in_code_fence {
            if line == "```" {
                in_code_fence = false;
            }
            // Skip all lines inside the fenced block body.
            continue;
        }
        // Skip "[\u2014 excerpt limited...]" footer lines produced when a
        // file was truncated by the byte cap.
        if (line.starts_with("[file: ") || line.starts_with("[dir: "))
            && line.contains('\u{2014}')
            && line.ends_with(']')
        {
            continue;
        }
        // [file: path] header — emit @path, consume the opening ```text fence.
        if let Some(rest) = line
            .strip_prefix("[file: ")
            .and_then(|r| r.strip_suffix(']'))
        {
            result.push(format!("@{rest}"));
            if matches!(lines.next(), Some(l) if l == "```text") {
                in_code_fence = true;
            }
            continue;
        }
        // [dir: path] header — emit @path/, consume the opening ```text fence.
        if let Some(rest) = line
            .strip_prefix("[dir: ")
            .and_then(|r| r.strip_suffix(']'))
        {
            result.push(format!("@{rest}/"));
            if matches!(lines.next(), Some(l) if l == "```text") {
                in_code_fence = true;
            }
            continue;
        }
        result.push(line.to_string());
    }
    result.join("\n")
}

fn word_wrap_transcript_row(row: &TranscriptRow, cols: usize) -> Vec<String> {
    // Structural rows (tool headers, evidence, etc.) are never word-wrapped.
    if is_structural_transcript_row(row) {
        return vec![row.as_display_str().to_string()];
    }
    let text = row.as_display_str();
    word_wrap_plain_row(text, cols)
}

fn word_wrap_plain_row(line: &str, cols: usize) -> Vec<String> {
    if cols < 4 || line.is_empty() {
        return vec![line.to_string()];
    }
    if display_width(line) <= cols || is_structural_plain_str(line) {
        return vec![line.to_string()];
    }
    word_wrap_to_cols(line, cols)
}

/// Returns true if `row` should not be word-wrapped.
fn is_structural_transcript_row(row: &TranscriptRow) -> bool {
    match row {
        // These variants are always structural: indented or short by design.
        TranscriptRow::ToolHeader(_)
        | TranscriptRow::ToolDetail(_)
        | TranscriptRow::Evidence(_)
        | TranscriptRow::Error(_)
        | TranscriptRow::UserInput(_)
        | TranscriptRow::WaitingPlaceholder(_) => true,
        // Plain runtime strings: use the older marker-string heuristic.
        TranscriptRow::Plain(s) => is_structural_plain_str(s),
        // AssistantText is wrapped normally.
        TranscriptRow::AssistantText { .. } => false,
    }
}

fn is_structural_plain_str(line: &str) -> bool {
    if line.starts_with('[')
        || line.starts_with("    ")
        || line.starts_with("  ")
        || line.starts_with("```")
        || (line.starts_with("--- ") && line.ends_with(" ---"))
        || is_horizontal_rule(line)
        || line.starts_with("> ")
        || is_telemetry_summary(line)
        || is_inline_telemetry_summary(line)
        || line.starts_with("# ")
        || line.starts_with("## ")
        || line.starts_with("### ")
        || line.starts_with("- ")
        || line.starts_with("* ")
    {
        return true;
    }

    parse_numbered_list_item(line).is_some()
}

fn word_wrap_to_cols(text: &str, cols: usize) -> Vec<String> {
    if cols == 0 {
        return vec![text.to_string()];
    }

    let options = textwrap::Options::new(cols)
        .word_splitter(textwrap::WordSplitter::NoHyphenation)
        .break_words(false);
    let wrapped = textwrap::wrap(text, &options);
    if wrapped.is_empty() {
        return vec![String::new()];
    }

    wrapped
        .into_iter()
        .map(|segment| {
            let segment = segment.into_owned();
            if display_width(&segment) > cols {
                truncate_line(&segment, cols)
            } else {
                segment
            }
        })
        .collect()
}

fn is_inline_telemetry_summary(line: &str) -> bool {
    line.starts_with('[') && line.ends_with(']') && line.contains("total:")
}

fn is_telemetry_summary(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('[')
        && trimmed.ends_with(']')
        && (trimmed.contains("total:") || trimmed.contains("ttft:"))
}

pub(crate) fn parse_numbered_list_item(line: &str) -> Option<(&str, &str, usize)> {
    let bytes = line.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_digit() {
        return None;
    }

    let mut index = 0usize;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == 0 || index >= bytes.len().saturating_sub(1) {
        return None;
    }

    if (bytes[index] == b'.' || bytes[index] == b')')
        && index + 1 < bytes.len()
        && bytes[index + 1] == b' '
    {
        let prefix = &line[..index + 2];
        let rest = &line[index + 2..];
        Some((prefix, rest, display_width(prefix)))
    } else {
        None
    }
}

pub(crate) fn is_horizontal_rule(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < 3 {
        return false;
    }

    let first = trimmed.as_bytes()[0];
    if first != b'-' && first != b'*' && first != b'_' {
        return false;
    }

    let mark_count = trimmed.chars().filter(|ch| *ch as u8 == first).count();
    let space_count = trimmed.chars().filter(|ch| *ch == ' ').count();
    mark_count >= 3 && mark_count + space_count == trimmed.len()
}
