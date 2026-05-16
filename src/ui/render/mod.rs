use crate::ui::input_metrics::{
    cursor_row_col, display_width, visual_row_count, visual_window_start, wrap_input_lines,
};
use crate::ui::layout::{preferred_four_region_input_rows_for_content, split_compact_task_layout};
use crate::ui::tui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Clear, Paragraph, Wrap},
};

pub use crate::ui::layout::MAX_INPUT_PANE_ROWS;

pub enum OverlayModal<'a> {
    PatchApprove {
        patch_preview: &'a str,
        scroll_offset: usize,
    },
    ToolPermission {
        tool_name: &'a str,
        input_preview: &'a str,
        auto_approve_enabled: bool,
    },
    MemoryClear,
}

pub fn input_visual_rows(input: &str, width: usize) -> usize {
    visual_row_count(input, width)
}

fn saturating_row_count_u16(rows: usize) -> u16 {
    rows.min(u16::MAX as usize) as u16
}

pub fn render_input(
    frame: &mut Frame<'_>,
    area: Rect,
    input: &str,
    cursor_byte: usize,
    show_cursor: bool,
) {
    render_input_with_actions(frame, area, input, cursor_byte, show_cursor, &[]);
}

fn render_input_with_actions(
    frame: &mut Frame<'_>,
    area: Rect,
    input: &str,
    cursor_byte: usize,
    show_cursor: bool,
    actions: &[Line<'static>],
) {
    if area.height == 0 || area.width <= 2 {
        return;
    }

    let action_rows = saturating_row_count_u16(actions.len()).min(area.height.saturating_sub(1));
    let (action_area, inner) = if action_rows == 0 {
        (Rect::new(area.x, area.y, area.width, 0), area)
    } else {
        let [action_area, inner] =
            Layout::vertical([Constraint::Length(action_rows), Constraint::Fill(1)]).areas(area);
        (action_area, inner)
    };

    if action_area.height > 0 {
        let visible_actions = &actions[..action_area.height as usize];
        let paragraph = match visible_actions {
            [line] => Paragraph::new(line.clone()),
            _ => Paragraph::new(visible_actions.to_vec()),
        }
        .style(Style::new().bg(Color::Rgb(24, 24, 24)))
        .alignment(Alignment::Left);
        frame.render_widget(paragraph, action_area);
    }

    if inner.height == 0 {
        return;
    }

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
                Style::new()
                    .fg(Color::Gray)
                    .bg(Color::Rgb(24, 24, 24))
                    .dim(),
            )
            .wrap(Wrap { trim: false }),
        inner,
    );

    if show_cursor {
        let cursor_y = inner
            .y
            .saturating_add(cursor_row.saturating_sub(window_start) as u16);
        let cursor_x = inner
            .x
            .saturating_add(2 + cursor_col as u16)
            .min(inner.x.saturating_add(inner.width.saturating_sub(1)));
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

pub fn render_task_input(frame: &mut Frame<'_>, area: Rect, input: &str, cursor_byte: usize) {
    render_input(frame, area, input, cursor_byte, true);
}

pub fn render_messages(frame: &mut Frame<'_>, area: Rect, messages: &[String]) {
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

    let total_lines = body.len();
    let scroll = total_lines.saturating_sub(inner.height as usize);
    let paragraph =
        Paragraph::new(Text::from_iter(body)).scroll((scroll.min(u16::MAX as usize) as u16, 0));
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
        Span::styled(line_prefix, Style::new().dark_gray().dim()),
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

fn diff_line_color(kind: DiffLineKind, other_color: Color) -> Color {
    match kind {
        DiffLineKind::Added => Color::Green,
        DiffLineKind::Removed => Color::Red,
        DiffLineKind::Header => Color::Cyan,
        DiffLineKind::Other => other_color,
    }
}

fn history_row_style(row: &str) -> Style {
    Style::new().fg(diff_line_color(classify_diff_line(row), Color::White))
}

pub fn render_status_line(frame: &mut Frame<'_>, area: Rect, status: &str) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let text = truncate_line(status, area.width as usize);
    frame.render_widget(
        Paragraph::new(Text::raw(text)).style(Style::new().dark_gray()),
        area,
    );
}

fn task_fork_action_line() -> Line<'static> {
    Line::from(vec![
        Span::styled(
            " Fork ",
            Style::new().fg(Color::Black).bg(Color::Cyan).bold(),
        ),
        Span::styled(" Alt+F", Style::new().cyan().dim()),
        Span::styled(" new task-id", Style::new().dark_gray()),
    ])
}

pub fn render_task_layout(frame: &mut Frame<'_>, state: &crate::app::TaskViewProjection) {
    let frame_area = frame.area();
    let input_width = frame_area.width.saturating_sub(2).max(1) as usize;
    let input_rows = preferred_four_region_input_rows_for_content(
        frame_area.height,
        saturating_row_count_u16(
            input_visual_rows(&state.composer_text, input_width).saturating_add(1),
        ),
    );

    const STATUS_ROWS: u16 = 1;
    let available_output = frame_area
        .height
        .saturating_sub(STATUS_ROWS)
        .saturating_sub(input_rows);
    let (output_start, output_end) = task_output_window_with_total(
        state,
        state.expanded_output_rows.len(),
        available_output as usize,
    );
    let visible_rows = (output_end - output_start) as u16;
    let layout = split_compact_task_layout(frame_area, STATUS_ROWS, visible_rows, input_rows);
    frame.render_widget(Clear, frame_area);
    render_status_line(frame, layout.header, &state.status_line);

    let output_lines: Vec<Line> = state.expanded_output_rows[output_start..output_end]
        .iter()
        .map(|row| transcript_output_line(row))
        .collect();
    let output_area = task_output_render_area(state, layout.output, output_lines.len());
    if output_area.height > 0 {
        frame.render_widget(Paragraph::new(Text::from(output_lines)), output_area);
    }

    render_input_with_actions(
        frame,
        layout.input,
        &state.composer_text,
        state.composer_cursor,
        state.composer_focused,
        &[task_fork_action_line()],
    );

    render_picker_overlay(frame, layout.input, &state.picker_overlay);
}

fn render_picker_overlay(
    frame: &mut Frame<'_>,
    composer_area: Rect,
    lines: &[crate::app::PickerOverlayLine],
) {
    if lines.is_empty() || composer_area.width < 4 {
        return;
    }

    let frame_area = frame.area();
    let above = composer_area.y.saturating_sub(frame_area.y);
    let below =
        (frame_area.y + frame_area.height).saturating_sub(composer_area.y + composer_area.height);

    let (available_height, render_below) = if below > above {
        (below, true)
    } else {
        (above, false)
    };

    if available_height < 3 {
        return;
    }

    let max_visible_rows = available_height.saturating_sub(2).max(1) as usize;
    let visible_rows = lines.len().min(max_visible_rows);
    let content_width = lines
        .iter()
        .take(visible_rows)
        .map(|line| display_width(&line.text))
        .max()
        .unwrap_or(0)
        .max(1);
    let width = (content_width as u16)
        .saturating_add(2)
        .min(composer_area.width.max(4));
    let height = (visible_rows as u16)
        .saturating_add(2)
        .min(available_height);
    let y = if render_below {
        composer_area.y + composer_area.height
    } else {
        composer_area.y.saturating_sub(height)
    };
    let area = Rect::new(composer_area.x, y, width, height);

    frame.render_widget(Clear, area);
    let block = Block::bordered().style(Style::new().dark_gray());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let rendered = lines
        .iter()
        .take(inner.height as usize)
        .map(|line| {
            let text = truncate_line(&line.text, inner.width as usize);
            if line.selected {
                Line::from(text).style(Style::new().cyan().bold())
            } else if line.text.starts_with('[') || line.text.starts_with('/') {
                Line::from(text).style(Style::new().dark_gray().dim())
            } else {
                Line::from(text).style(Style::new().fg(Color::White))
            }
        })
        .collect::<Vec<_>>();

    frame.render_widget(Paragraph::new(rendered), inner);
}

pub fn render_overlay_modal(frame: &mut Frame<'_>, modal: OverlayModal<'_>) {
    render_overlay_modal_in_area(frame, frame.area(), modal);
}

pub fn render_overlay_modal_in_area(frame: &mut Frame<'_>, anchor: Rect, modal: OverlayModal<'_>) {
    if anchor.width == 0 || anchor.height == 0 {
        return;
    }

    let preferred_height = modal_preferred_height(&modal);
    let area = centered_modal_area(anchor, preferred_height);
    let provisional_outer = Block::bordered();
    let provisional_inner = provisional_outer.inner(area);
    let [provisional_body_area, _provisional_shortcuts_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(provisional_inner);
    let body_block = Block::bordered().title_top(Line::from("Body"));
    let body_inner = body_block.inner(provisional_body_area);
    let (title, accent, body, shortcuts) = modal_content(modal, body_inner.height as usize);

    frame.render_widget(Clear, area);
    let outer = Block::bordered()
        .title_top(Line::from(title))
        .style(Style::new().fg(accent));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let [body_area, shortcuts_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);
    let body_block = Block::bordered().title_top(Line::from("Body"));
    let body_inner = body_block.inner(body_area);
    frame.render_widget(body_block, body_area);

    frame.render_widget(
        Paragraph::new(Text::from_iter(body))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false }),
        body_inner,
    );

    frame.render_widget(
        Paragraph::new(shortcuts)
            .alignment(Alignment::Center)
            .style(Style::new().dark_gray()),
        shortcuts_area,
    );
}

fn modal_preferred_height(modal: &OverlayModal<'_>) -> u16 {
    match modal {
        OverlayModal::PatchApprove { .. } => 18,
        OverlayModal::ToolPermission { .. } => 14,
        OverlayModal::MemoryClear => 10,
    }
}

fn modal_content(
    modal: OverlayModal<'_>,
    body_viewport_rows: usize,
) -> (&'static str, Color, Vec<Line<'static>>, &'static str) {
    match modal {
        OverlayModal::PatchApprove {
            patch_preview,
            scroll_offset,
        } => {
            let lines: Vec<&str> = patch_preview.lines().collect();
            let start = scroll_offset.min(lines.len().saturating_sub(1));
            let visible = body_viewport_rows.saturating_sub(4).max(1);
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
            body.push(Line::styled("Patch", Style::new().bold()));
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
                Style::new().bold(),
            ));
            if auto_approve_enabled {
                body.push(Line::styled(
                    "session auto-approve is ON",
                    Style::new().fg(Color::Green).bold(),
                ));
            }
            body.push(Line::from(""));
            body.push(Line::styled("Preview", Style::new().yellow().bold()));
            let preview_lines: Vec<&str> = input_preview.lines().collect();
            let reserved_rows = if auto_approve_enabled { 4 } else { 3 };
            let max_preview_lines = body_viewport_rows.saturating_sub(reserved_rows).max(1);
            for line in preview_lines.iter().take(max_preview_lines) {
                body.push(Line::from(line.to_string()));
            }
            if preview_lines.len() > max_preview_lines {
                body.push(Line::styled(
                    format!(
                        "... ({} more lines)",
                        preview_lines.len() - max_preview_lines
                    ),
                    Style::new().dark_gray().dim(),
                ));
            }
            (
                "Tool Permission",
                Color::Yellow,
                body,
                "1 yes   2 allow this session   3/esc cancel",
            )
        }
        OverlayModal::MemoryClear => (
            "Memory Clear",
            Color::Yellow,
            vec![
                Line::styled("Clear all saved memory notes?", Style::new().bold()),
                Line::from("Type y or yes to confirm."),
                Line::from("Any deny action leaves the notes file unchanged."),
            ],
            "y/yes confirm   n/esc cancel",
        ),
    }
}

fn styled_diff_line(line: &str) -> Line<'static> {
    Line::styled(
        line.to_string(),
        Style::new().fg(diff_line_color(classify_diff_line(line), Color::White)),
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

mod markdown;
mod transcript;
pub(crate) use markdown::markdown_to_inline_line;
pub(crate) use transcript::*;

#[cfg(test)]
mod tests;
