use crate::app::TaskLayoutState;
use crate::status_contract::{
    is_waiting_placeholder, pending_status_label, status_tone, StatusTone,
};
use crate::ui::input_metrics::{
    char_display_width, cursor_row_col, truncate_to_display_width, visual_row_count,
    visual_window_start, wrap_input_lines,
};
use crate::ui::layout::{preferred_four_region_input_rows_for_content, split_four_region_layout};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

pub use crate::ui::layout::MAX_INPUT_PANE_ROWS;

pub enum OverlayModal<'a> {
    PatchApprove {
        patch_preview: &'a str,
        scroll_offset: usize,
        viewport_rows: usize,
    },
    ToolPermission {
        tool_name: &'a str,
        input_preview: &'a str,
        auto_approve_enabled: bool,
    },
}

pub fn input_visual_rows(input: &str, width: usize) -> usize {
    visual_row_count(input, width)
}

pub fn render_input(frame: &mut Frame<'_>, area: Rect, input: &str, cursor_byte: usize) {
    if area.height == 0 || area.width <= 2 {
        return;
    }
    let inner = area;

    let input_width = inner.width.saturating_sub(2).max(1) as usize;
    let lines = wrap_input_lines(input, input_width);
    let (cursor_row, cursor_col) = cursor_row_col(input, cursor_byte, input_width);
    let visible_rows = inner.height as usize;
    let window_start = visual_window_start(cursor_row, visible_rows);

    let mut rendered = Vec::with_capacity(visible_rows);
    for offset in 0..visible_rows {
        let row_index = window_start + offset;
        let prefix = if row_index == 0 { "> " } else { "  " };
        let line = lines.get(row_index).cloned().unwrap_or_default();
        rendered.push(Line::from(format!("{prefix}{line}")));
    }

    frame.render_widget(
        Paragraph::new(rendered)
            .style(
                Style::default()
                    .fg(Color::Gray)
                    .bg(Color::Rgb(24, 24, 24))
                    .add_modifier(Modifier::DIM),
            )
            .wrap(Wrap { trim: false }),
        inner,
    );

    let cursor_y = inner
        .y
        .saturating_add(cursor_row.saturating_sub(window_start) as u16);
    let cursor_x = inner
        .x
        .saturating_add(2 + cursor_col as u16)
        .min(inner.x.saturating_add(inner.width.saturating_sub(1)));
    frame.set_cursor_position((cursor_x, cursor_y));
}

pub fn render_task_input(
    frame: &mut Frame<'_>,
    area: Rect,
    input: &str,
    cursor_byte: usize,
    footer: &str,
) {
    if area.height == 0 || area.width <= 2 {
        return;
    }

    let footer_lines = footer
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let footer_height = footer_lines
        .len()
        .min(area.height.saturating_sub(1) as usize) as u16;
    let input_height = area.height.saturating_sub(footer_height).max(1);
    let input_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: input_height,
    };
    render_input(frame, input_area, input, cursor_byte);

    if footer_height == 0 {
        return;
    }

    let footer_area = Rect {
        x: area.x,
        y: area.y.saturating_add(input_height),
        width: area.width,
        height: footer_height,
    };
    let rows = footer_lines
        .into_iter()
        .take(footer_height as usize)
        .map(Line::from)
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(rows).style(
            Style::default()
                .fg(Color::Yellow)
                .bg(Color::Rgb(24, 24, 24))
                .add_modifier(Modifier::DIM),
        ),
        footer_area,
    );
}

pub fn render_messages(frame: &mut Frame<'_>, area: Rect, messages: &[String], scroll: usize) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let inner = area;

    let logical_rows = expand_history_rows(messages);
    let line_number_width = logical_rows.len().max(1).to_string().len();
    let content_width = history_content_width(inner.width, line_number_width);
    let mut body: Vec<Line<'static>> = Vec::new();
    for (index, row) in logical_rows.iter().enumerate() {
        let row_style = history_row_style(row);
        let wrapped_segments = wrap_input_lines(row, content_width);
        for (segment_index, segment) in wrapped_segments.iter().enumerate() {
            body.push(format_history_row_segment(
                index + 1,
                line_number_width,
                segment,
                row_style,
                segment_index == 0,
            ));
        }
    }

    let paragraph =
        Paragraph::new(Text::from(body)).scroll((scroll.min(u16::MAX as usize) as u16, 0));
    frame.render_widget(paragraph, inner);
}

pub fn history_visual_line_count(messages: &[String], content_width: usize) -> usize {
    if messages.is_empty() {
        return 0;
    }

    let content_width = content_width.max(1);
    expand_history_rows(messages)
        .iter()
        .map(|row| wrap_input_lines(row, content_width).len().max(1))
        .sum()
}

pub fn history_content_width_for_area(messages: &[String], area: Rect) -> usize {
    let row_count = expand_history_rows(messages).len().max(1);
    let line_number_width = row_count.to_string().len();
    history_content_width(area.width, line_number_width)
}

fn history_content_width(area_width: u16, line_number_width: usize) -> usize {
    area_width
        .saturating_sub((line_number_width + 3) as u16)
        .max(1) as usize
}

fn expand_history_rows(messages: &[String]) -> Vec<String> {
    if messages.is_empty() {
        return Vec::new();
    }

    let mut rows = Vec::new();
    for message in messages {
        if message.is_empty() {
            rows.push(String::new());
            continue;
        }
        rows.extend(message.split('\n').map(ToOwned::to_owned));
    }
    rows
}

fn format_history_row_segment(
    line_number: usize,
    line_number_width: usize,
    row: &str,
    style: Style,
    show_line_number: bool,
) -> Line<'static> {
    let line_prefix = if show_line_number {
        format!("{line_number:>line_number_width$} | ")
    } else {
        format!("{:>line_number_width$} | ", "")
    };
    Line::from(vec![
        Span::styled(
            line_prefix,
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ),
        Span::styled(row.to_string(), style),
    ])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiffLineKind {
    Added,
    Removed,
    Header,
    Other,
}

fn classify_diff_line(line: &str) -> DiffLineKind {
    if line.starts_with('+') && !line.starts_with("+++") {
        DiffLineKind::Added
    } else if line.starts_with('-') && !line.starts_with("---") {
        DiffLineKind::Removed
    } else if line.starts_with("@@") || line.starts_with("diff --git") || line.starts_with("index ")
    {
        DiffLineKind::Header
    } else {
        DiffLineKind::Other
    }
}

/// Map a `DiffLineKind` to a display color, using `other_color` as the
/// fallback for `DiffLineKind::Other`.  Centralizes the Added/Removed/Header
/// mapping that is shared by `history_row_style` and `styled_diff_line`.
fn diff_line_color(kind: DiffLineKind, other_color: Color) -> Color {
    match kind {
        DiffLineKind::Added => Color::Green,
        DiffLineKind::Removed => Color::Red,
        DiffLineKind::Header => Color::Cyan,
        DiffLineKind::Other => other_color,
    }
}

fn history_row_style(row: &str) -> Style {
    Style::default().fg(diff_line_color(classify_diff_line(row), Color::White))
}

pub fn render_status_line(frame: &mut Frame<'_>, area: Rect, status: &str) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let text = truncate_line(status, area.width as usize);
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

/// Render the legacy four-region task-first layout.
///
/// The activity pane uses structured `timeline_entries` to render
/// the selected timeline entry highlighted with its detail shown
/// in the output/inspector pane.
pub fn render_task_layout(frame: &mut Frame<'_>, state: &TaskLayoutState) {
    let input_width = frame.area().width.saturating_sub(2).max(1) as usize;
    let layout = split_four_region_layout(
        frame.area(),
        0,
        preferred_four_region_input_rows_for_content(
            frame.area().height,
            input_visual_rows(&state.composer_text, input_width) as u16,
        ),
    );
    frame.render_widget(Clear, frame.area());

    // --- Output / Inspector pane ---
    let (output_start, output_end) = task_output_window(state, layout.output.height as usize);
    let output_lines: Vec<Line> = state.output_rows[output_start..output_end]
        .iter()
        .map(|row| transcript_output_line(row))
        .collect();
    let output_area = task_output_render_area(state, layout.output, output_lines.len());
    if output_area.height > 0 {
        frame.render_widget(
            Paragraph::new(Text::from(output_lines)).wrap(Wrap { trim: false }),
            output_area,
        );
    }

    // --- Input pane ---
    if state.pending_approval.is_none() {
        render_task_input(
            frame,
            layout.input,
            &state.composer_text,
            state.composer_cursor,
            &state.input_hint,
        );
    } else {
        frame.render_widget(
            Paragraph::new(state.input_hint.clone()).wrap(Wrap { trim: false }),
            layout.input,
        );
    }
}

#[cfg(test)]
/// Render a single timeline entry with lifecycle-based colour coding
/// and an optional selection indicator.
fn render_timeline_entry(entry: &crate::app::TimelineEntry, is_selected: bool) -> Line<'static> {
    use crate::app::StepLifecycle;

    let (prefix, prefix_color) = match entry.lifecycle {
        StepLifecycle::Completed => ("[ok]", Color::Green),
        StepLifecycle::Failed => ("[!]", Color::Red),
        StepLifecycle::Running => ("[->]", Color::Magenta),
        StepLifecycle::AwaitingApproval => ("[?]", Color::Yellow),
        StepLifecycle::Approved => ("[v]", Color::Green),
        StepLifecycle::UserInput => (">", Color::DarkGray),
        StepLifecycle::CommandSession => ("[$$]", Color::Magenta),
    };

    let selector = if is_selected { "> " } else { "  " };
    let body_style = if is_selected {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(prefix_color)
    };

    Line::from(vec![
        Span::styled(
            selector.to_string(),
            if is_selected {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
        Span::styled(
            prefix.to_string(),
            Style::default()
                .fg(prefix_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {}", entry.label), body_style),
    ])
}

pub fn render_overlay_modal(frame: &mut Frame<'_>, modal: OverlayModal<'_>) {
    render_overlay_modal_in_area(frame, frame.area(), modal);
}

pub fn render_overlay_modal_in_area(frame: &mut Frame<'_>, anchor: Rect, modal: OverlayModal<'_>) {
    if anchor.width == 0 || anchor.height == 0 {
        return;
    }

    let (title, accent, body, shortcuts) = modal_content(modal);
    let preferred_height = (body.len() + 8) as u16;
    let area = centered_modal_area(anchor, preferred_height);
    frame.render_widget(Clear, area);

    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(Style::default().fg(accent));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);
    let body_area = vertical[0];
    let shortcuts_area = vertical[1];

    let body_block = Block::default().borders(Borders::ALL).title("Body");
    let body_inner = body_block.inner(body_area);
    frame.render_widget(body_block, body_area);

    frame.render_widget(
        Paragraph::new(Text::from(body))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false }),
        body_inner,
    );

    frame.render_widget(
        Paragraph::new(shortcuts)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray)),
        shortcuts_area,
    );
}

fn modal_content(
    modal: OverlayModal<'_>,
) -> (&'static str, Color, Vec<Line<'static>>, &'static str) {
    match modal {
        OverlayModal::PatchApprove {
            patch_preview,
            scroll_offset,
            viewport_rows,
        } => {
            let lines: Vec<&str> = patch_preview.lines().collect();
            let start = scroll_offset.min(lines.len().saturating_sub(1));
            let visible = viewport_rows.max(1);
            let end = (start + visible).min(lines.len());

            let mut body = Vec::new();
            body.push(Line::from("Review and approve patch application."));
            body.push(Line::from(format!(
                "showing {}-{} of {}",
                if lines.is_empty() { 0 } else { start + 1 },
                end,
                lines.len()
            )));
            body.push(Line::from(""));
            body.push(Line::styled(
                "Patch",
                Style::default().add_modifier(Modifier::BOLD),
            ));
            for line in lines.iter().skip(start).take(visible) {
                body.push(styled_diff_line(line));
            }

            (
                "Patch Approve",
                Color::Blue,
                body,
                "y/1 approve   n/3/esc reject   up/down/pgup/pgdn/home/end scroll",
            )
        }
        OverlayModal::ToolPermission {
            tool_name,
            input_preview,
            auto_approve_enabled,
        } => {
            let mut body = Vec::new();
            body.push(Line::styled(
                format!("Tool: {tool_name}"),
                Style::default().add_modifier(Modifier::BOLD),
            ));
            if auto_approve_enabled {
                body.push(Line::styled(
                    "session auto-approve is ON",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            body.push(Line::from(""));
            body.push(Line::styled(
                "Preview",
                Style::default().add_modifier(Modifier::BOLD),
            ));
            let preview_lines: Vec<&str> = input_preview.lines().collect();
            let max_preview_lines = 6;
            for line in preview_lines.iter().take(max_preview_lines) {
                body.push(Line::from(line.to_string()));
            }
            if preview_lines.len() > max_preview_lines {
                body.push(Line::styled(
                    format!(
                        "... ({} more lines)",
                        preview_lines.len() - max_preview_lines
                    ),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                ));
            }
            (
                "Tool Permission",
                Color::Yellow,
                body,
                "1 yes   2 allow this session   3/esc cancel",
            )
        }
    }
}

fn styled_diff_line(line: &str) -> Line<'static> {
    Line::styled(
        line.to_string(),
        Style::default().fg(diff_line_color(classify_diff_line(line), Color::White)),
    )
}

fn centered_modal_area(size: Rect, preferred_height: u16) -> Rect {
    let width = size.width.clamp(44, 96);
    let max_height = size.height.clamp(8, 24);
    let height = preferred_height.clamp(8, max_height);
    let x = size.x + (size.width.saturating_sub(width)) / 2;
    let y = size.y + (size.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

/// Render a single pipeline activity row with prefix-based colour coding.
/// Matches the prefixes used in transcript content lines:
///   `[ok]`  → green   (completed step)
///   `[!]`   → red     (failed/error step)
///   `[->]`  → violet  (in-progress orchestration step)
///   `[?]`   → yellow  (approval request)
///   `> …`   → dim gray (user prompt echo)
fn pipeline_activity_line(row: &str) -> Line<'static> {
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

fn transcript_output_line(row: &str) -> Line<'static> {
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
    } else {
        Line::from(Span::styled(
            row.to_string(),
            Style::default().fg(Color::White),
        ))
    }
}

fn structured_transcript_line(
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
    if let Some((prefix, rest, _)) = crate::ui::draw::transcript::parse_numbered_list_item(trimmed)
    {
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

fn json_transcript_line(
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

fn looks_like_json_line(text: &str) -> bool {
    text.trim_start().starts_with('{')
        || text.trim_start().starts_with('[')
        || text.trim_start().starts_with('}')
        || text.trim_start().starts_with(']')
        || text.contains("\":")
}

fn diff_style(line: &str) -> Option<(Color, bool)> {
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

fn parse_command_session_started(line: &str) -> Option<(String, Option<String>)> {
    let rest = line.strip_prefix("[command session started")?;
    let (meta, command) = rest.split_once("] ")?;
    let pid = meta
        .strip_prefix(" pid=")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    Some((command.to_string(), pid))
}

fn split_tool_summary(text: &str) -> Option<(Vec<&str>, &str)> {
    let segments: Vec<&str> = text.split(" \u{00b7} ").collect();
    let (status, leading) = segments.split_last()?;
    if leading.is_empty() || status_tone(status).is_none() {
        return None;
    }
    Some((leading.to_vec(), *status))
}

fn tool_status_style(status: &str) -> Style {
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

fn truncate_line(input: &str, width: usize) -> String {
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

fn task_output_window(state: &TaskLayoutState, viewport_height: usize) -> (usize, usize) {
    const INSPECTOR_VIEWPORT_ROWS: usize = 6;

    let total = state.output_rows.len();
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

fn task_output_render_area(state: &TaskLayoutState, area: Rect, visible_rows: usize) -> Rect {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn all_modals_use_unified_renderer() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        let modals = [
            OverlayModal::PatchApprove {
                patch_preview: "diff --git a/src/app/mod.rs b/src/app/mod.rs",
                scroll_offset: 0,
                viewport_rows: 8,
            },
            OverlayModal::ToolPermission {
                tool_name: "exec_command",
                input_preview: "echo hi",
                auto_approve_enabled: false,
            },
        ];

        for modal in modals {
            terminal
                .draw(|frame| render_overlay_modal(frame, modal))
                .expect("renderer should support every modal class");
        }
    }

    #[test]
    fn visual_window_start_scrolls_once_cursor_exceeds_visible_rows() {
        assert_eq!(visual_window_start(0, 4), 0);
        assert_eq!(visual_window_start(3, 4), 0);
        assert_eq!(visual_window_start(4, 4), 1);
        assert_eq!(visual_window_start(7, 4), 4);
    }

    #[test]
    fn diff_line_semantics_are_styled_by_prefix() {
        let add = styled_diff_line("+added");
        let del = styled_diff_line("-removed");
        let hunk = styled_diff_line("@@ -1 +1 @@");
        let ctx = styled_diff_line(" context");

        assert_eq!(add.style.fg, Some(Color::Green));
        assert_eq!(del.style.fg, Some(Color::Red));
        assert_eq!(hunk.style.fg, Some(Color::Cyan));
        assert_eq!(ctx.style.fg, Some(Color::White));
    }

    #[test]
    fn history_visual_line_count_tracks_embedded_newlines() {
        let messages = vec![
            "first".to_string(),
            "line-a\nline-b".to_string(),
            String::new(),
        ];
        assert_eq!(history_visual_line_count(&messages, 80), 4);
    }

    #[test]
    fn history_visual_line_count_tracks_wrapped_rows() {
        let messages = vec!["123456".to_string()];
        assert_eq!(history_visual_line_count(&messages, 3), 2);
    }

    #[test]
    fn history_row_style_marks_diff_rows() {
        assert_eq!(history_row_style("+add").fg, Some(Color::Green));
        assert_eq!(history_row_style("-del").fg, Some(Color::Red));
        assert_eq!(history_row_style("@@ -1 +1 @@").fg, Some(Color::Cyan));
        assert_eq!(history_row_style("plain text").fg, Some(Color::White));
    }

    #[test]
    fn classify_diff_line_keeps_header_markers_consistent() {
        assert_eq!(classify_diff_line("diff --git a b"), DiffLineKind::Header);
        assert_eq!(classify_diff_line("index 123..456"), DiffLineKind::Header);
        assert_eq!(classify_diff_line("@@ -1 +1 @@"), DiffLineKind::Header);
    }

    #[test]
    fn test_diff_line_color_maps_markers_consistently() {
        // Verify the shared helper maps Added/Removed/Header consistently,
        // regardless of which other_color is passed as the fallback.
        assert_eq!(
            diff_line_color(DiffLineKind::Added, Color::White),
            Color::Green
        );
        assert_eq!(
            diff_line_color(DiffLineKind::Added, Color::Gray),
            Color::Green
        );
        assert_eq!(
            diff_line_color(DiffLineKind::Removed, Color::White),
            Color::Red
        );
        assert_eq!(
            diff_line_color(DiffLineKind::Removed, Color::Gray),
            Color::Red
        );
        assert_eq!(
            diff_line_color(DiffLineKind::Header, Color::White),
            Color::Cyan
        );
        assert_eq!(
            diff_line_color(DiffLineKind::Header, Color::Gray),
            Color::Cyan
        );
        // Other respects the caller's choice of fallback color.
        assert_eq!(
            diff_line_color(DiffLineKind::Other, Color::White),
            Color::White
        );
        assert_eq!(
            diff_line_color(DiffLineKind::Other, Color::Gray),
            Color::Gray
        );
        // Both plain history rows and diff context lines keep the default white.
        assert_eq!(history_row_style("+add").fg, Some(Color::Green));
        assert_eq!(history_row_style("plain").fg, Some(Color::White));
        assert_eq!(styled_diff_line("+add").style.fg, Some(Color::Green));
        assert_eq!(styled_diff_line(" ctx").style.fg, Some(Color::White));
    }

    #[test]
    fn render_timeline_entry_normalizes_prefix_spacing() {
        let approval = render_timeline_entry(
            &crate::app::TimelineEntry {
                step_id: 1,
                lifecycle: crate::app::StepLifecycle::AwaitingApproval,
                label: "ApplyPatch: src/main.rs".into(),
                detail: String::new(),
                session_id: None,
            },
            true,
        );
        let user_input = render_timeline_entry(
            &crate::app::TimelineEntry {
                step_id: 0,
                lifecycle: crate::app::StepLifecycle::UserInput,
                label: "ship it".into(),
                detail: String::new(),
                session_id: None,
            },
            true,
        );

        let approval_text: String = approval
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        let user_input_text: String = user_input
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert_eq!(approval_text, "> [?] ApplyPatch: src/main.rs");
        assert_eq!(user_input_text, "> > ship it");
    }

    #[test]
    fn render_timeline_entry_gives_approved_a_distinct_prefix() {
        let approved = render_timeline_entry(
            &crate::app::TimelineEntry {
                step_id: 1,
                lifecycle: crate::app::StepLifecycle::Approved,
                label: "ApplyPatch: approved".into(),
                detail: String::new(),
                session_id: None,
            },
            false,
        );
        let completed = render_timeline_entry(
            &crate::app::TimelineEntry {
                step_id: 2,
                lifecycle: crate::app::StepLifecycle::Completed,
                label: "ApplyPatch: done".into(),
                detail: String::new(),
                session_id: None,
            },
            false,
        );

        let approved_text: String = approved
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        let completed_text: String = completed
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert!(approved_text.starts_with("  [v]"));
        assert!(completed_text.starts_with("  [ok]"));
    }

    #[test]
    fn test_changed_files_and_live_approval_prompt_render() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = crate::app::TaskLayoutState {
            task_id: "task-001".into(),
            status_line: "AwaitingApproval".into(),
            telemetry: crate::app::TaskTelemetryState::default(),
            timeline_entries: vec![crate::app::TimelineEntry {
                step_id: 1,
                lifecycle: crate::app::StepLifecycle::AwaitingApproval,
                label: "ApplyPatch: src/main.rs".into(),
                detail: "Tool: ApplyPatch\nFile: src/main.rs".into(),
                session_id: None,
            }],
            selected_step: 0,
            total_steps: 1,
            output_title: "Transcript".into(),
            output_rows: vec![],
            output_scroll_offset: 0,
            output_scroll_anchor: crate::app::OutputScrollAnchor::Bottom,
            changed_files: vec!["src/main.rs".into()],
            pending_approval: Some("ApplyPatch: src/main.rs".into()),
            input_hint: "ApplyPatch: src/main.rs\n[y/n/s] ".into(),
            composer_text: String::new(),
            composer_cursor: 0,
            composer_focused: true,
            follow_mode: true,
            picker_overlay: vec![],
        };

        terminal.draw(|f| render_task_layout(f, &state)).unwrap();

        let rendered = terminal.backend().buffer().clone();
        let flat = rendered
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .join("");
        assert!(
            flat.contains("src/main.rs"),
            "changed file must appear in rendered output"
        );
        assert!(
            flat.contains("ApplyPatch"),
            "approval prompt must appear in rendered output"
        );
        assert!(
            flat.contains("[y/n/s]"),
            "approval choices must appear in rendered output"
        );
    }

    #[test]
    fn task_layout_keeps_output_surface_primary_when_steps_are_pending() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = crate::app::TaskLayoutState {
            task_id: "task-002".into(),
            status_line: "Running".into(),
            telemetry: crate::app::TaskTelemetryState::default(),
            timeline_entries: vec![
                crate::app::TimelineEntry {
                    step_id: 1,
                    lifecycle: crate::app::StepLifecycle::Completed,
                    label: "read_file: ok".into(),
                    detail: "Tool: read_file\nOutcome: ok".into(),
                    session_id: None,
                },
                crate::app::TimelineEntry {
                    step_id: 2,
                    lifecycle: crate::app::StepLifecycle::Running,
                    label: "validate: Mapping adjacent sectors...".into(),
                    detail: "Tool: validate\nInput: ...".into(),
                    session_id: None,
                },
            ],
            selected_step: 1,
            total_steps: 2,
            output_title: "Inspector".into(),
            output_rows: vec!["streamed output".into()],
            output_scroll_offset: 0,
            output_scroll_anchor: crate::app::OutputScrollAnchor::Top,
            changed_files: vec![],
            pending_approval: None,
            input_hint: "> ".into(),
            composer_text: String::new(),
            composer_cursor: 0,
            composer_focused: true,
            follow_mode: true,
            picker_overlay: vec![],
        };

        terminal.draw(|f| render_task_layout(f, &state)).unwrap();

        let rendered = terminal.backend().buffer().clone();
        let flat = rendered
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .join("");
        assert!(
            flat.contains("streamed output"),
            "the output pane should remain primary when steps are pending"
        );
        assert!(
            !flat.contains("Orchestrating")
                && !flat.contains("validate: Mapping adjacent sectors..."),
            "the fallback surface should not render a separate top activity pane"
        );
    }

    #[test]
    fn task_layout_uses_full_body_for_transcript_on_tall_terminals() {
        let backend = TestBackend::new(80, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let timeline_entries = (0..12)
            .map(|index| crate::app::TimelineEntry {
                step_id: index as u64,
                lifecycle: crate::app::StepLifecycle::Completed,
                label: format!("step_{index} · State synchronized."),
                detail: format!("Tool: step_{index}"),
                session_id: None,
            })
            .collect();
        let state = crate::app::TaskLayoutState {
            task_id: "task-003".into(),
            status_line: "Running".into(),
            telemetry: crate::app::TaskTelemetryState::default(),
            timeline_entries,
            selected_step: 8,
            total_steps: 12,
            output_title: "Inspector".into(),
            output_rows: vec!["output".into()],
            output_scroll_offset: 0,
            output_scroll_anchor: crate::app::OutputScrollAnchor::Top,
            changed_files: vec![],
            pending_approval: None,
            input_hint: "> ".into(),
            composer_text: String::new(),
            composer_cursor: 0,
            composer_focused: true,
            follow_mode: true,
            picker_overlay: vec![],
        };

        terminal.draw(|f| render_task_layout(f, &state)).unwrap();

        let rendered = terminal.backend().buffer().clone();
        let flat = rendered
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .join("");
        assert!(
            flat.contains("output"),
            "the fallback transcript should use the available body space"
        );
    }

    #[test]
    fn task_layout_without_changed_files_bottom_anchors_short_transcript() {
        let backend = TestBackend::new(60, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = crate::app::TaskLayoutState {
            task_id: "task-004".into(),
            status_line: "Running".into(),
            telemetry: crate::app::TaskTelemetryState::default(),
            timeline_entries: vec![crate::app::TimelineEntry {
                step_id: 1,
                lifecycle: crate::app::StepLifecycle::Completed,
                label: "step_1 · State synchronized.".into(),
                detail: "Tool: step_1".into(),
                session_id: None,
            }],
            selected_step: 0,
            total_steps: 1,
            output_title: "Transcript".into(),
            output_rows: vec!["body row".into()],
            output_scroll_offset: 0,
            output_scroll_anchor: crate::app::OutputScrollAnchor::Bottom,
            changed_files: vec![],
            pending_approval: None,
            input_hint: "> ".into(),
            composer_text: String::new(),
            composer_cursor: 0,
            composer_focused: true,
            follow_mode: true,
            picker_overlay: vec![],
        };

        terminal.draw(|f| render_task_layout(f, &state)).unwrap();

        let rendered = terminal.backend().buffer().clone();
        let rows: Vec<String> = rendered
            .content()
            .chunks(60)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect();
        let first_non_empty = rows
            .iter()
            .position(|row| row.contains("body row"))
            .expect("body row must appear in rendered output");
        assert!(
            first_non_empty > 0,
            "short transcript should hug the prompt edge instead of starting at the top row"
        );
    }

    #[test]
    fn transcript_output_line_styles_paragraph_markers() {
        let tool = transcript_output_line("[tool] read_file · State synchronized.");
        let detail = transcript_output_line("[detail] Scope: Read file content");
        let evidence = transcript_output_line("[evidence] Outcome: ok");

        let tool_text: String = tool
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        let detail_text: String = detail
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        let evidence_text: String = evidence
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert!(tool_text.starts_with("  \u{2726} "));
        assert_eq!(detail_text, "    Scope: Read file content");
        assert!(evidence_text.starts_with("      \u{2727} "));
    }

    #[test]
    fn transcript_output_line_styles_waiting_placeholder_as_progress() {
        let waiting = transcript_output_line("[thinking] Mapping adjacent sectors...");

        let waiting_text: String = waiting
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert_eq!(waiting_text, "  ⋯ Mapping adjacent sectors...");
        assert_eq!(waiting.spans[1].style.fg, Some(Color::Magenta));
        assert!(waiting.spans[1].style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn transcript_output_line_defaults_plain_text_to_white() {
        let plain = transcript_output_line("plain response");

        assert_eq!(plain.spans.len(), 1);
        assert_eq!(plain.spans[0].content.as_ref(), "plain response");
        assert_eq!(plain.spans[0].style.fg, Some(Color::White));
    }

    #[test]
    fn transcript_output_line_keeps_command_session_progress_status_with_pid() {
        let command = transcript_output_line("[command session started pid=42] cargo test");

        let command_text: String = command
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert!(command_text.contains("pid 42"));
        assert!(command_text.contains("Mapping adjacent sectors..."));
        assert_eq!(
            command.spans.last().map(|span| span.style.fg),
            Some(Some(Color::Magenta))
        );
    }

    #[test]
    fn transcript_output_line_highlights_tool_target_and_failed_status() {
        let tool = transcript_output_line("[tool] write_file · src/lib.rs · failed");

        assert_eq!(tool.spans[1].content.as_ref(), "write_file");
        assert_eq!(tool.spans[1].style.fg, Some(Color::White));
        assert_eq!(tool.spans[3].content.as_ref(), "src/lib.rs");
        assert_eq!(tool.spans[3].style.fg, Some(Color::Gray));
        assert_eq!(tool.spans[5].content.as_ref(), "failed");
        assert_eq!(tool.spans[5].style.fg, Some(Color::Red));
        assert!(tool.spans[5].style.add_modifier.contains(Modifier::BOLD));
    }
}
