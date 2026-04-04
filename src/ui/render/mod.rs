use crate::app::TaskLayoutState;
use crate::ui::input_metrics::{
    cursor_row_col, visual_row_count, visual_window_start, wrap_input_lines,
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
    } else if let Some(marker) = numbered_diff_marker(line) {
        match marker {
            '+' => DiffLineKind::Added,
            '-' => DiffLineKind::Removed,
            _ => DiffLineKind::Other,
        }
    } else if line.starts_with("@@") || line.starts_with("diff --git") || line.starts_with("index ")
    {
        DiffLineKind::Header
    } else {
        DiffLineKind::Other
    }
}

fn numbered_diff_marker(line: &str) -> Option<char> {
    let bytes = line.as_bytes();
    let mut index = 0usize;

    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == 0 || index >= bytes.len() {
        return None;
    }
    if bytes[index] != b' ' {
        return None;
    }

    while index < bytes.len() && bytes[index] == b' ' {
        index += 1;
    }
    let marker = *bytes.get(index)?;
    if marker != b'+' && marker != b'-' {
        return None;
    }
    if bytes.get(index + 1).copied() != Some(b' ') {
        return None;
    }
    Some(marker as char)
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
    let expanded_output_rows =
        crate::ui::draw::expand_rows_for_display(&state.output_rows, layout.output.width);
    let (output_start, output_end) =
        task_output_window(state, layout.output.width, layout.output.height as usize);
    let output_lines: Vec<Line> = expanded_output_rows[output_start..output_end]
        .iter()
        .map(|row| transcript_output_line(row))
        .collect();
    let output_area = task_output_render_area(state, layout.output, output_lines.len());
    if output_area.height > 0 {
        frame.render_widget(Paragraph::new(Text::from(output_lines)), output_area);
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
mod markdown;
mod transcript;
pub(crate) use markdown::markdown_to_inline_line;
pub(crate) use transcript::*;

#[cfg(test)]
mod tests;
