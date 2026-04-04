use crate::app::TaskLayoutState;
use crate::status_contract::{
    is_waiting_placeholder, pending_status_label, status_tone, StatusTone,
};
use crate::ui::input_metrics::{char_display_width, truncate_to_display_width};
use ansi_to_tui::IntoText;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

pub(crate) fn pipeline_activity_line(row: &str) -> Line<'static> {
    if let Some(rest) = row.strip_prefix("[ok]") {
        Line::from(vec![
            Span::styled(
                "[ok]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(rest.to_string(), Style::default().fg(Color::Green)),
        ])
    } else if let Some(rest) = row.strip_prefix("[!]") {
        Line::from(vec![
            Span::styled(
                "[!] ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(rest.to_string(), Style::default().fg(Color::Red)),
        ])
    } else if let Some(rest) = row.strip_prefix("[->]") {
        Line::from(vec![
            Span::styled(
                "[->]",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(rest.to_string(), Style::default().fg(Color::Magenta)),
        ])
    } else if let Some(rest) = row.strip_prefix("[?]") {
        Line::from(vec![
            Span::styled(
                "[?] ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(rest.to_string(), Style::default().fg(Color::Yellow)),
        ])
    } else if let Some(rest) = row.strip_prefix("> ") {
        Line::from(vec![
            Span::styled(
                "> ",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            ),
            Span::styled(
                rest.to_string(),
                Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
            ),
        ])
    } else {
        Line::from(Span::styled(
            row.to_string(),
            Style::default().fg(Color::White),
        ))
    }
}

pub(crate) fn transcript_output_line(row: &str) -> Line<'static> {
    if let Some(rest) = row.strip_prefix("[turn] ") {
        Line::from(vec![
            Span::styled(
                "─── ✦ ",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            ),
            Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    } else if is_waiting_placeholder(row) {
        Line::from(vec![
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
        ])
    } else if let Some(rest) = row.strip_prefix("[thinking] ") {
        Line::from(vec![
            Span::styled(
                "  ⋯ ",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::DIM),
            ),
        ])
    } else if let Some(rest) = row.strip_prefix("[thinking_detail] ") {
        Line::styled(
            format!("    {rest}"),
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::ITALIC | Modifier::DIM),
        )
    } else if let Some(rest) = row.strip_prefix("[approval] ") {
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
    } else if let Some(rest) = row.strip_prefix("[error] ") {
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
    } else if let Some(rest) = row.strip_prefix("[tool] ") {
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
            Line::from(spans)
        } else {
            spans.push(Span::styled(
                rest.to_string(),
                Style::default().fg(Color::White),
            ));
            Line::from(spans)
        }
    } else if let Some(rest) = row.strip_prefix("[detail] ") {
        structured_transcript_line(rest, "    ", None)
    } else if let Some(rest) = row.strip_prefix("[evidence] ") {
        structured_transcript_line(rest, "      ", Some("\u{2727} "))
    } else if let Some((command, pid)) = parse_command_session_started(row) {
        let summary = pid
            .map(|pid| {
                format!(
                    "command session · {command} · pid {pid} · {}",
                    pending_status_label()
                )
            })
            .unwrap_or_else(|| format!("command session · {command} · {}", pending_status_label()));
        transcript_output_line(&format!("[tool] {summary}"))
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
        transcript_output_line(&format!("[error] {rest}"))
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
    } else if row.starts_with("[ok]")
        || row.starts_with("[!]")
        || row.starts_with("[->]")
        || row.starts_with("[?]")
        || row.starts_with("> ")
    {
        pipeline_activity_line(row)
    } else if row.contains('\x1b') {
        // Parse ANSI escape sequences into styled ratatui spans.
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
        // The fallback renderer can safely style single logical markdown rows
        // here, but fenced blocks need to be parsed before row expansion.
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
    if let Some((prefix, rest, _)) = crate::ui::draw::parse_numbered_list_item(trimmed) {
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
    let (meta, command) = rest.split_once("] ")?;
    let pid = meta
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
    let mut truncated = false;

    for ch in input.chars() {
        let ch_width = char_display_width(ch);
        if used + ch_width > width {
            truncated = true;
            break;
        }
        out.push(ch);
        used += ch_width;
    }

    if truncated && width >= 4 {
        out = truncate_to_display_width(&out, width - 3);
        out.push_str("...");
    }
    out
}

pub(crate) fn task_output_window(
    state: &TaskLayoutState,
    viewport_width: u16,
    viewport_height: usize,
) -> (usize, usize) {
    const INSPECTOR_VIEWPORT_ROWS: usize = 6;

    let total = crate::ui::draw::expand_rows_for_display(&state.output_rows, viewport_width).len();
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
    state: &TaskLayoutState,
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

    if state.output_scroll_anchor == crate::app::OutputScrollAnchor::Bottom
        && visible_rows < area.height as usize
    {
        return Rect {
            x: area.x,
            y: area.y + area.height.saturating_sub(visible_rows as u16),
            width: area.width,
            height: visible_rows as u16,
        };
    }

    Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: visible_rows.min(area.height as usize) as u16,
    }
}
