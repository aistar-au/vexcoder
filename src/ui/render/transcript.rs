use std::borrow::Cow;

use crate::app::{TaskViewProjection, TranscriptRow};
use crate::status_contract::{StatusTone, pending_status_label, status_tone};
use crate::ui::input_metrics::{char_display_width, display_width, truncate_to_display_width};
use crate::ui::tui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use ansi_to_tui::IntoText;

const ESC: char = '\x1b';

pub(crate) fn transcript_output_line(row: &TranscriptRow) -> Line<'static> {
    match row {
        TranscriptRow::WaitingPlaceholder(_) => Line::from(vec![
            Span::styled(
                "  ⋯ ",
                Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                pending_status_label().to_string(),
                Style::new().fg(Color::Magenta).add_modifier(Modifier::DIM),
            ),
        ]),
        TranscriptRow::Error(rest) => Line::from(vec![
            Span::styled(
                "  ✖ ",
                Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                rest.clone(),
                Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
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
                Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM),
            ),
            Span::styled(
                text.clone(),
                Style::new().fg(Color::Gray).add_modifier(Modifier::DIM),
            ),
        ]),
        TranscriptRow::AssistantText { text, .. } => render_assistant_text(text),
        TranscriptRow::Plain(s) => render_plain_row(s),
    }
}

fn render_tool_header(rest: &str) -> Line<'static> {
    let mut spans = vec![Span::styled(
        "  \u{2726} ",
        Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD),
    )];
    if let Some((leading, status)) = split_tool_summary(rest) {
        for (index, segment) in leading.iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled(
                    " \u{00b7} ",
                    Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM),
                ));
            }
            spans.push(Span::styled(
                (*segment).to_string(),
                if index == 0 {
                    Style::new().fg(Color::White)
                } else {
                    Style::new().fg(Color::Gray)
                },
            ));
        }
        spans.push(Span::styled(
            " \u{00b7} ",
            Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM),
        ));
        spans.push(Span::styled(status.to_string(), tool_status_style(status)));
    } else {
        spans.push(Span::styled(
            rest.to_string(),
            Style::new().fg(Color::White),
        ));
    }
    Line::from(spans)
}

fn render_assistant_text(text: &str) -> Line<'static> {
    let default = Style::new().fg(Color::White);
    if contains_ansi_escape(text) {
        ansi_to_inline_line(text, default)
    } else if looks_like_inline_markdown(text) {
        super::markdown_to_inline_line(text)
            .unwrap_or_else(|| Line::from(Span::raw(text.to_string())))
    } else {
        Line::from(Span::styled(text.to_string(), default))
    }
}

pub(crate) fn contains_ansi_escape(text: &str) -> bool {
    text.contains(ESC)
}

pub(crate) fn ansi_to_inline_line(text: &str, fallback_style: Style) -> Line<'static> {
    let cleaned = strip_overstrike(text);
    match cleaned.as_ref().into_text() {
        Ok(parsed) => collapse_text_to_line(parsed.lines, &cleaned, fallback_style),
        Err(_) => Line::from(Span::styled(cleaned.into_owned(), fallback_style)),
    }
}

fn collapse_text_to_line(
    lines: Vec<Line<'static>>,
    fallback: &str,
    fallback_style: Style,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for line in lines {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.extend(line.spans);
    }
    if spans.is_empty() {
        Line::from(Span::styled(fallback.to_string(), fallback_style))
    } else {
        Line::from(spans)
    }
}

fn strip_overstrike(text: &str) -> Cow<'_, str> {
    if !text.contains('\r') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut first = true;
    for segment in text.split('\n') {
        if !first {
            out.push('\n');
        }
        first = false;
        let last = segment.rsplit('\r').next().unwrap_or(segment);
        out.push_str(last);
    }
    Cow::Owned(out)
}

pub(crate) fn visible_display_width(text: &str) -> usize {
    if !contains_ansi_escape(text) {
        return display_width(text);
    }
    let cleaned = strip_overstrike(text);
    match cleaned.as_ref().into_text() {
        Ok(parsed) => parsed
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| display_width(span.content.as_ref()))
                    .sum::<usize>()
            })
            .max()
            .unwrap_or(0),
        Err(_) => display_width(cleaned.as_ref()),
    }
}

fn render_plain_row(row: &str) -> Line<'static> {
    if let Some(line) = render_plain_row_dispatch(row) {
        return line;
    }
    if contains_ansi_escape(row) {
        return ansi_to_inline_line(row, Style::new().fg(Color::White));
    }
    if looks_like_inline_markdown(row) {
        return super::markdown_to_inline_line(row)
            .unwrap_or_else(|| Line::from(Span::raw(row.to_string())));
    }
    Line::from(Span::styled(row.to_string(), Style::new().fg(Color::White)))
}

fn render_plain_row_dispatch(row: &str) -> Option<Line<'static>> {
    if let Some(rest) = row.strip_prefix("[approval] ") {
        return Some(line_with_marker(
            "  ? ",
            rest,
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(rest) = row.strip_prefix("[approval_detail] ") {
        return Some(Line::styled(
            format!("    {rest}"),
            Style::new().fg(Color::Gray).add_modifier(Modifier::DIM),
        ));
    }
    if let Some((command, pid)) = parse_command_session_started(row) {
        let summary = match pid {
            Some(pid) => format!(
                "command session · {command} · pid {pid} · {}",
                pending_status_label()
            ),
            None => format!("command session · {command} · {}", pending_status_label()),
        };
        return Some(render_tool_header(&summary));
    }
    if let Some(rest) = row.strip_prefix("[command session exit: ") {
        return Some(structured_transcript_line(
            &format!("Exit: {}", rest.trim_end_matches(']')),
            "    ",
            None,
        ));
    }
    if row == "[command session cancelled]" {
        return Some(structured_transcript_line(
            "Status: cancelled",
            "    ",
            None,
        ));
    }
    if row == "[command session cancellation requested]" {
        return Some(structured_transcript_line(
            "Status: cancellation requested",
            "    ",
            None,
        ));
    }
    if let Some(rest) = row.strip_prefix("[command session] error: ") {
        return Some(line_with_marker(
            "  ✖ ",
            rest,
            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(rest) = row.strip_prefix("[stderr] ") {
        return Some(line_with_marker(
            "      \u{2727} ",
            rest,
            Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM),
            Style::new().fg(Color::Red).add_modifier(Modifier::DIM),
        ));
    }
    None
}

fn line_with_marker(
    marker: &'static str,
    body: &str,
    marker_style: Style,
    body_style: Style,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(marker.to_string(), marker_style),
        Span::styled(body.to_string(), body_style),
    ])
}

pub(crate) fn looks_like_inline_markdown(row: &str) -> bool {
    if row.contains("```") {
        return false;
    }
    let heading_mark_count = row.bytes().take_while(|byte| *byte == b'#').count();
    let after_heading_marks = row.as_bytes().get(heading_mark_count).copied();
    if (1..=6).contains(&heading_mark_count)
        && matches!(after_heading_marks, None | Some(b' ' | b'\t'))
    {
        return true;
    }
    if has_paired_marker(row, "**") {
        return true;
    }
    has_paired_backtick(row)
}

fn has_paired_marker(row: &str, marker: &str) -> bool {
    let mut count = 0usize;
    let mut rest = row;
    while let Some(idx) = rest.find(marker) {
        count += 1;
        rest = &rest[idx + marker.len()..];
    }
    count >= 2
}

fn has_paired_backtick(row: &str) -> bool {
    row.bytes().filter(|b| *b == b'`').count() >= 2
}

pub(crate) fn structured_transcript_line(
    row: &str,
    indent: &'static str,
    marker: Option<&'static str>,
) -> Line<'static> {
    let trimmed = row.trim_start();
    if let Some((color, bold)) = diff_style(trimmed) {
        let mut style = Style::new().fg(color).add_modifier(Modifier::DIM);
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        let mut spans = vec![Span::styled(
            format!("{indent}{}", marker.unwrap_or("")),
            Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM),
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
                Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM),
            ),
            Span::styled("• ".to_string(), Style::new().fg(Color::Yellow)),
            Span::styled(
                rest.to_string(),
                Style::new().fg(Color::Gray).add_modifier(Modifier::DIM),
            ),
        ]);
    }
    if let Some((prefix, rest, _)) = parse_numbered_list_item(trimmed) {
        return Line::from(vec![
            Span::styled(
                format!("{indent}{}", marker.unwrap_or("")),
                Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM),
            ),
            Span::styled(prefix.to_string(), Style::new().fg(Color::Yellow)),
            Span::styled(
                rest.to_string(),
                Style::new().fg(Color::Gray).add_modifier(Modifier::DIM),
            ),
        ]);
    }
    Line::from(vec![
        Span::styled(
            format!("{indent}{}", marker.unwrap_or("")),
            Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM),
        ),
        Span::styled(
            trimmed.to_string(),
            Style::new().fg(Color::Gray).add_modifier(Modifier::DIM),
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
        Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM),
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
            spans.push(Span::styled(token, Style::new().fg(color)));
            continue;
        }
        let style = match ch {
            '0'..='9' | '-' => Style::new().fg(Color::Yellow),
            't' | 'f' | 'n' => Style::new().fg(Color::Magenta),
            _ => Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM),
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
    Style::new()
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
                    expanded.push(row.clone_with_text(wrapped));
                }
            }
        }
    }
    expanded
}

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

            continue;
        }

        if (line.starts_with("[file: ") || line.starts_with("[dir: "))
            && line.contains('\u{2014}')
            && line.ends_with(']')
        {
            continue;
        }

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
    if matches!(row, TranscriptRow::WaitingPlaceholder(_)) {
        return vec![row.as_display_str().to_string()];
    }
    let text = row.as_display_str();
    if contains_ansi_escape(text) {
        return vec![text.to_string()];
    }
    let wrap_cols = transcript_row_wrap_width(row, cols);
    if visible_display_width(text) <= wrap_cols || is_structural_transcript_row(row) {
        return vec![text.to_string()];
    }
    word_wrap_plain_row(text, wrap_cols)
}

fn word_wrap_plain_row(line: &str, cols: usize) -> Vec<String> {
    if cols < 4 || line.is_empty() {
        return vec![line.to_string()];
    }
    if contains_ansi_escape(line) {
        return vec![line.to_string()];
    }
    if visible_display_width(line) <= cols || is_structural_plain_str(line) {
        return vec![line.to_string()];
    }
    word_wrap_to_cols(line, cols)
}

fn is_structural_transcript_row(row: &TranscriptRow) -> bool {
    match row {
        TranscriptRow::WaitingPlaceholder(_) => true,

        TranscriptRow::ToolHeader(_)
        | TranscriptRow::ToolDetail(_)
        | TranscriptRow::Evidence(_)
        | TranscriptRow::Error(_)
        | TranscriptRow::UserInput(_) => false,

        TranscriptRow::Plain(s) => is_structural_plain_str(s),

        TranscriptRow::AssistantText { .. } => false,
    }
}

fn transcript_row_wrap_width(row: &TranscriptRow, cols: usize) -> usize {
    cols.saturating_sub(transcript_row_prefix_width(row)).max(1)
}

fn transcript_row_prefix_width(row: &TranscriptRow) -> usize {
    match row {
        TranscriptRow::ToolHeader(_) => display_width("  \u{2726} "),
        TranscriptRow::ToolDetail(_) => display_width("    "),
        TranscriptRow::Evidence(_) => display_width("      \u{2727} "),
        TranscriptRow::Error(_) => display_width("  \u{2716} "),
        TranscriptRow::UserInput(_) => display_width("> "),
        TranscriptRow::Plain(line) => plain_row_prefix_width(line),
        TranscriptRow::AssistantText { .. } | TranscriptRow::WaitingPlaceholder(_) => 0,
    }
}

fn plain_row_prefix_width(line: &str) -> usize {
    if line.strip_prefix("[approval] ").is_some() {
        display_width("  ? ")
    } else if line.strip_prefix("[approval_detail] ").is_some() {
        display_width("    ")
    } else if parse_command_session_started(line).is_some() {
        display_width("  \u{2726} ")
    } else if line.strip_prefix("[command session exit: ").is_some()
        || line == "[command session cancelled]"
        || line == "[command session cancellation requested]"
    {
        display_width("    ")
    } else if line.strip_prefix("[command session] error: ").is_some() {
        display_width("  \u{2716} ")
    } else if line.strip_prefix("[stderr] ").is_some() {
        display_width("      \u{2727} ")
    } else {
        0
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered_width(row: &TranscriptRow) -> usize {
        let line = transcript_output_line(row);
        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        display_width(&text)
    }

    #[test]
    fn expand_rows_wraps_tool_detail_to_visual_width() {
        let rows = vec![TranscriptRow::ToolDetail(
            "Input: query: <missing> adjacent sectors after finding release workflow".to_string(),
        )];

        let expanded = expand_rows_for_display(&rows, 24);

        assert!(
            expanded.len() > 1,
            "tool detail should wrap for inline scrollback"
        );
        assert!(expanded.iter().all(|row| rendered_width(row) <= 24));
    }

    #[test]
    fn expand_rows_wraps_tool_header_to_visual_width() {
        let rows = vec![TranscriptRow::ToolHeader(
            "search_files · .github/workflows/release.yml · Response complete.".to_string(),
        )];

        let expanded = expand_rows_for_display(&rows, 28);

        assert!(
            expanded.len() > 1,
            "tool header should wrap for inline scrollback"
        );
        assert!(expanded.iter().all(|row| rendered_width(row) <= 28));
    }

    #[test]
    fn strip_overstrike_keeps_only_last_segment_per_line() {
        assert_eq!(strip_overstrike("plain"), "plain");
        assert_eq!(strip_overstrike("first\rsecond"), "second");
        assert_eq!(
            strip_overstrike("downloading 50%\rdownloading 75%\rdone"),
            "done"
        );
        assert_eq!(strip_overstrike("a\rb\nc\rd"), "b\nd");
    }

    #[test]
    fn ansi_to_inline_line_preserves_content_after_cr_overstrike() {
        let raw = "stale\rfresh \x1b[32mvalue\x1b[0m";
        let line = ansi_to_inline_line(raw, Style::new());
        let visible: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(
            !visible.contains("stale"),
            "overstruck prefix must be dropped, got {visible:?}"
        );
        assert!(
            visible.contains("fresh") && visible.contains("value"),
            "post-CR content must survive, got {visible:?}"
        );
    }

    #[test]
    fn ansi_to_inline_line_collapses_multi_line_decoded_text() {
        let raw = "alpha\x1b[31m\nbeta\x1b[0m";
        let line = ansi_to_inline_line(raw, Style::new());
        let visible: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(
            visible.contains("alpha") && visible.contains("beta"),
            "both decoded lines must be retained on a single rendered row, got {visible:?}"
        );
    }

    #[test]
    fn visible_display_width_ignores_ansi_bytes() {
        let plain = "hello";
        let styled = "\x1b[31mhello\x1b[0m";
        assert_eq!(visible_display_width(plain), 5);
        assert_eq!(visible_display_width(styled), 5);
    }

    #[test]
    fn word_wrap_plain_row_does_not_split_ansi_sequences() {
        let raw = "\x1b[31mthis is a long styled fragment that would otherwise be wrapped\x1b[0m";
        let wrapped = word_wrap_plain_row(raw, 10);
        assert_eq!(
            wrapped,
            vec![raw.to_string()],
            "ANSI-bearing rows must not be wrapped (would split escape sequences)"
        );
    }

    #[test]
    fn looks_like_inline_markdown_rejects_unpaired_marks() {
        assert!(!looks_like_inline_markdown("note: foo ** still open"));
        assert!(!looks_like_inline_markdown("count = `len"));
        assert!(!looks_like_inline_markdown("#define MAX 10"));
        assert!(!looks_like_inline_markdown("####### Too deep"));
    }

    #[test]
    fn looks_like_inline_markdown_accepts_real_markdown() {
        assert!(looks_like_inline_markdown("# Heading"));
        assert!(looks_like_inline_markdown("## Subheading"));
        assert!(looks_like_inline_markdown("###\tTabbed subheading"));
        assert!(looks_like_inline_markdown("###### Deep subheading"));
        assert!(looks_like_inline_markdown("#"));
        assert!(looks_like_inline_markdown("see **this** part"));
        assert!(looks_like_inline_markdown("call `foo()` here"));
    }

    #[test]
    fn render_plain_row_dispatch_handles_known_prefixes() {
        assert!(render_plain_row_dispatch("[approval] confirm?").is_some());
        assert!(render_plain_row_dispatch("[approval_detail] details").is_some());
        assert!(render_plain_row_dispatch("[stderr] oops").is_some());
        assert!(render_plain_row_dispatch("[command session exit: 0]").is_some());
        assert!(render_plain_row_dispatch("[command session cancelled]").is_some());
        assert!(render_plain_row_dispatch("[command session] error: boom").is_some());
        assert!(render_plain_row_dispatch("ordinary line").is_none());
    }
}
