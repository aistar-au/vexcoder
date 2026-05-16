use anyhow::Result;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::{
    FileMentionPickerState, PickerOverlayLine, SlashPickerMatch, SlashPickerState, TuiMode,
};
use crate::runtime::frontend::{FrontendAdapter, InputOccurrence, ScrollAction, ScrollTarget};
use crate::runtime::mode::RuntimeMode;
use crate::startup::{
    STARTUP_NOISE_GUARD, looks_like_session_output, should_ignore_startup_paste_text,
};
use crate::ui::editor::{InputAction, InputEditor, file_mention_range};
use crate::ui::layout::{
    Rect, preferred_four_region_input_rows_for_content, split_three_pane_layout,
};
use crate::ui::render::{
    OverlayModal, expand_rows_for_display, history_content_width_for_area, input_visual_rows,
    render_input, render_messages, render_overlay_modal_in_area, render_status_line,
    render_task_layout, transcript_output_line,
};
use crate::ui::tui::input::{self, Event, KeyCode, KeyModifiers, KeyStroke, KeyStrokeKind};
use crate::ui::tui::widgets::Clear;

pub struct ManagedTuiFrontend {
    tui: crate::tui_handle::TuiHandle,
    quit: bool,
    editor: InputEditor,
    started_at: Instant,
    last_file_picker_prefix: String,
    selected_file_hint: usize,
    dismissed_file_picker: Option<(String, Range<usize>)>,
    cached_file_picker: Option<(String, usize, Option<FileMentionPickerState>)>,
    selected_slash_hint: usize,
    last_slash_picker_prefix: String,
    dismissed_slash_picker: bool,
    cached_slash_picker: Option<(String, usize, Option<SlashPickerState>)>,
    inline_scrollback: Option<InlineScrollbackSnapshot>,
}

#[derive(Clone, Debug)]
struct InlineScrollbackSnapshot {
    task_id: String,
    area: Rect,
    visible_start: usize,
    expanded_output_rows: Arc<[crate::app::TranscriptRow]>,
}

impl ManagedTuiFrontend {
    pub fn new() -> Result<Self> {
        let tui = crate::tui_handle::setup()?;
        Self::flush_startup_signals();
        Ok(Self {
            tui,
            quit: false,
            editor: InputEditor::new(),
            started_at: Instant::now(),
            last_file_picker_prefix: String::new(),
            selected_file_hint: 0,
            dismissed_file_picker: None,
            cached_file_picker: None,
            selected_slash_hint: 0,
            last_slash_picker_prefix: String::new(),
            dismissed_slash_picker: false,
            cached_slash_picker: None,
            inline_scrollback: None,
        })
    }

    fn maybe_insert_task_scrollback(
        &mut self,
        state: &crate::app::TaskLayoutState,
        expanded_output_rows: Arc<[crate::app::TranscriptRow]>,
        area: Rect,
    ) {
        let current = state
            .follow_mode
            .then(|| build_inline_scrollback_snapshot(state, expanded_output_rows, area));

        if let (Some(previous), Some(current)) = (self.inline_scrollback.as_ref(), current.as_ref())
            && let Some((start, end)) = inline_rows_to_insert(previous, current)
        {
            let lines = previous.expanded_output_rows[start..end]
                .iter()
                .map(transcript_output_line)
                .collect::<Vec<_>>();
            let _ = self.tui.insert_before_lines(lines);
        }

        self.inline_scrollback = current;
    }

    fn current_file_picker(&mut self, mode: &TuiMode) -> Option<FileMentionPickerState> {
        let input = self.editor.buffer();
        let cursor = self.editor.cursor();
        if file_picker_is_dismissed(self.dismissed_file_picker.as_ref(), input, cursor) {
            return None;
        }
        if let Some((cached_input, cached_cursor, cached_picker)) = &self.cached_file_picker
            && cached_input == input
            && *cached_cursor == cursor
        {
            return cached_picker.clone();
        }

        let picker = active_file_picker(mode, input, cursor);
        self.cached_file_picker = Some((input.to_string(), cursor, picker.clone()));
        picker
    }

    fn dismiss_current_file_picker(&mut self) {
        self.dismissed_file_picker = file_mention_range(self.editor.buffer(), self.editor.cursor())
            .map(|range| (self.editor.buffer().to_string(), range));
        self.cached_file_picker = None;
        self.last_file_picker_prefix.clear();
        self.selected_file_hint = 0;
    }

    fn current_slash_picker(&mut self, mode: &TuiMode) -> Option<SlashPickerState> {
        let input = self.editor.buffer();
        let cursor = self.editor.cursor();
        if self.dismissed_slash_picker && slash_prefix_token(input).is_some() {
            return None;
        }
        if let Some((cached_input, cached_cursor, cached_picker)) = &self.cached_slash_picker
            && cached_input == input
            && *cached_cursor == cursor
        {
            return cached_picker.clone();
        }
        let picker = active_slash_picker(mode, input);
        self.cached_slash_picker = Some((input.to_string(), cursor, picker.clone()));
        picker
    }

    fn dismiss_current_slash_picker(&mut self) {
        self.dismissed_slash_picker = true;
        self.cached_slash_picker = None;
        self.last_slash_picker_prefix.clear();
        self.selected_slash_hint = 0;
    }

    fn flush_startup_signals() {
        for _ in 0..1024 {
            match input::poll(Duration::from_millis(0)) {
                Ok(true) => {
                    if input::read().is_err() {
                        break;
                    }
                }
                Ok(false) | Err(_) => break,
            }
        }
    }

    fn startup_filter_active() -> bool {
        std::env::var("VEX_DISABLE_STARTUP_FILTER").as_deref() != Ok("1")
    }

    fn should_ignore_startup_paste(&self, text: &str) -> bool {
        Self::startup_filter_active()
            && should_ignore_startup_paste_text(
                text,
                self.started_at.elapsed() <= STARTUP_NOISE_GUARD,
            )
    }

    fn should_ignore_startup_submission(&self, text: &str) -> bool {
        Self::startup_filter_active()
            && self.started_at.elapsed() <= STARTUP_NOISE_GUARD
            && looks_like_session_output(text)
    }

    fn map_editor_action(&mut self, action: InputAction) -> Option<InputOccurrence> {
        match action {
            InputAction::None => None,
            InputAction::Interrupt => Some(InputOccurrence::Interrupt),
            InputAction::Quit => {
                self.quit = true;
                None
            }
            InputAction::Submit(value) => {
                if self.should_ignore_startup_submission(&value) {
                    None
                } else {
                    Some(InputOccurrence::Text(value))
                }
            }
        }
    }

    fn map_overlay_key(&mut self, key: KeyStroke) -> Option<InputOccurrence> {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(InputOccurrence::Interrupt)
            }
            KeyCode::Up => Some(InputOccurrence::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::LineUp,
            }),
            KeyCode::Down => Some(InputOccurrence::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::LineDown,
            }),
            KeyCode::PageUp => Some(InputOccurrence::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::PageUp(10),
            }),
            KeyCode::PageDown => Some(InputOccurrence::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::PageDown(10),
            }),
            KeyCode::Home => Some(InputOccurrence::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::Home,
            }),
            KeyCode::End => Some(InputOccurrence::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::End,
            }),
            KeyCode::Esc => Some(InputOccurrence::Text("esc".to_string())),
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                Some(InputOccurrence::Text(ch.to_string()))
            }
            _ => None,
        }
    }

    fn editor_visual_width(&self) -> usize {
        self.tui
            .size()
            .map(|size| size.width.saturating_sub(2).max(1) as usize)
            .unwrap_or(1)
    }

    fn map_regular_key(&mut self, key: KeyStroke, mode: &TuiMode) -> Option<InputOccurrence> {
        if key.modifiers.is_empty() {
            if let Some(picker) = self.current_slash_picker(mode) {
                let last_index = picker.matches.len().saturating_sub(1);
                self.selected_slash_hint = self.selected_slash_hint.min(last_index);
                match key.code {
                    KeyCode::Up if !picker.matches.is_empty() => {
                        self.selected_slash_hint = self.selected_slash_hint.saturating_sub(1);
                        return None;
                    }
                    KeyCode::Down if !picker.matches.is_empty() => {
                        self.selected_slash_hint = (self.selected_slash_hint + 1).min(last_index);
                        return None;
                    }
                    KeyCode::Enter if !picker.matches.is_empty() => {
                        let command = &picker.matches[self.selected_slash_hint].command;
                        apply_slash_picker_selection(&mut self.editor, command);
                        self.dismissed_slash_picker = false;
                        self.cached_slash_picker = None;
                        self.last_slash_picker_prefix.clear();
                        self.selected_slash_hint = 0;
                        return None;
                    }
                    KeyCode::Esc => {
                        self.dismiss_current_slash_picker();
                        return None;
                    }
                    _ => {
                        self.dismissed_slash_picker = false;
                    }
                }
            } else if slash_prefix_token(self.editor.buffer()).is_none() {
                self.dismissed_slash_picker = false;
            }

            if let Some(picker) = self.current_file_picker(mode) {
                let last_index = picker.matches.len().saturating_sub(1);
                self.selected_file_hint = self.selected_file_hint.min(last_index);
                match key.code {
                    KeyCode::Up if !picker.matches.is_empty() => {
                        self.selected_file_hint = self.selected_file_hint.saturating_sub(1);
                        return None;
                    }
                    KeyCode::Down if !picker.matches.is_empty() => {
                        self.selected_file_hint = (self.selected_file_hint + 1).min(last_index);
                        return None;
                    }
                    KeyCode::Enter if !picker.matches.is_empty() => {
                        let replacement = &picker.matches[self.selected_file_hint];
                        apply_file_picker_selection(&mut self.editor, &picker.range, replacement);
                        self.dismissed_file_picker = None;
                        self.cached_file_picker = None;
                        self.last_file_picker_prefix.clear();
                        self.selected_file_hint = 0;
                        return None;
                    }
                    KeyCode::Esc => {
                        self.dismiss_current_file_picker();
                        return None;
                    }
                    _ => {}
                }
            }
        }

        if let Some(action) = task_shortcut_occurrence(key) {
            return Some(action);
        }

        match key.code {
            KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => {
                Some(InputOccurrence::Scroll {
                    target: ScrollTarget::Timeline,
                    action: ScrollAction::LineUp,
                })
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
                Some(InputOccurrence::Scroll {
                    target: ScrollTarget::Timeline,
                    action: ScrollAction::LineDown,
                })
            }
            KeyCode::Home if key.modifiers.contains(KeyModifiers::ALT) => {
                Some(InputOccurrence::Scroll {
                    target: ScrollTarget::Timeline,
                    action: ScrollAction::Home,
                })
            }
            KeyCode::End if key.modifiers.contains(KeyModifiers::ALT) => {
                Some(InputOccurrence::Scroll {
                    target: ScrollTarget::Timeline,
                    action: ScrollAction::End,
                })
            }

            KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                Some(InputOccurrence::Scroll {
                    target: ScrollTarget::Timeline,
                    action: ScrollAction::LineUp,
                })
            }
            KeyCode::Tab => Some(InputOccurrence::Scroll {
                target: ScrollTarget::Timeline,
                action: ScrollAction::LineDown,
            }),
            KeyCode::PageUp => Some(InputOccurrence::Scroll {
                target: ScrollTarget::Output,
                action: ScrollAction::PageUp(10),
            }),
            KeyCode::PageDown => Some(InputOccurrence::Scroll {
                target: ScrollTarget::Output,
                action: ScrollAction::PageDown(10),
            }),
            KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(InputOccurrence::Scroll {
                    target: ScrollTarget::Output,
                    action: ScrollAction::LineUp,
                })
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(InputOccurrence::Scroll {
                    target: ScrollTarget::Output,
                    action: ScrollAction::LineDown,
                })
            }
            KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(InputOccurrence::Scroll {
                    target: ScrollTarget::Output,
                    action: ScrollAction::Home,
                })
            }
            KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(InputOccurrence::Scroll {
                    target: ScrollTarget::Output,
                    action: ScrollAction::End,
                })
            }
            KeyCode::Home if key.modifiers.is_empty() => {
                self.editor
                    .move_cursor_visual_home(self.editor_visual_width());
                None
            }
            KeyCode::End if key.modifiers.is_empty() => {
                self.editor
                    .move_cursor_visual_end(self.editor_visual_width());
                None
            }
            KeyCode::Up
                if key.modifiers.is_empty()
                    && self
                        .editor
                        .move_cursor_visual_up(self.editor_visual_width()) =>
            {
                None
            }
            KeyCode::Up if key.modifiers.is_empty() => {
                let action = self.editor.apply_key(key);
                self.map_editor_action(action)
            }
            KeyCode::Down
                if key.modifiers.is_empty()
                    && self
                        .editor
                        .move_cursor_visual_down(self.editor_visual_width()) =>
            {
                None
            }
            KeyCode::Down if key.modifiers.is_empty() => {
                let action = self.editor.apply_key(key);
                self.map_editor_action(action)
            }
            _ => {
                let action = self.editor.apply_key(key);
                self.map_editor_action(action)
            }
        }
    }

    fn map_command_session_key(&mut self, key: KeyStroke) -> Option<InputOccurrence> {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(InputOccurrence::Interrupt)
            }
            KeyCode::PageUp => Some(InputOccurrence::Scroll {
                target: ScrollTarget::Output,
                action: ScrollAction::PageUp(10),
            }),
            KeyCode::PageDown => Some(InputOccurrence::Scroll {
                target: ScrollTarget::Output,
                action: ScrollAction::PageDown(10),
            }),
            KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(InputOccurrence::Scroll {
                    target: ScrollTarget::Output,
                    action: ScrollAction::Home,
                })
            }
            KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(InputOccurrence::Scroll {
                    target: ScrollTarget::Output,
                    action: ScrollAction::End,
                })
            }
            KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(InputOccurrence::Scroll {
                    target: ScrollTarget::Output,
                    action: ScrollAction::LineUp,
                })
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(InputOccurrence::Scroll {
                    target: ScrollTarget::Output,
                    action: ScrollAction::LineDown,
                })
            }
            _ => None,
        }
    }

    fn build_picker_overlay(&mut self, mode: &TuiMode) -> Vec<PickerOverlayLine> {
        if let Some(picker) = self.current_slash_picker(mode) {
            if picker.matches.is_empty() {
                return Vec::new();
            }
            return build_slash_overlay(&picker.matches, self.selected_slash_hint);
        }

        if let Some(picker) = self.current_file_picker(mode) {
            return build_file_overlay(
                &picker.prefix,
                &picker.matches,
                picker.total_matches,
                self.selected_file_hint,
            );
        }

        Vec::new()
    }
}

const MAX_PICKER_OVERLAY_VISIBLE: usize = 12;

impl Drop for ManagedTuiFrontend {
    fn drop(&mut self) {
        let _ = crate::tui_handle::restore();
    }
}

impl FrontendAdapter<TuiMode> for ManagedTuiFrontend {
    fn poll_user_input(&mut self, mode: &TuiMode) -> Option<InputOccurrence> {
        if mode.quit_requested() {
            self.quit = true;
            return None;
        }

        let poll_ms = if mode.is_pulse_in_progress() { 1 } else { 16 };
        let Ok(has_input) = input::poll(Duration::from_millis(poll_ms)) else {
            self.quit = true;
            return None;
        };
        if !has_input {
            return None;
        }

        let Ok(ev) = input::read() else {
            self.quit = true;
            return None;
        };

        match ev {
            Event::Key(key) => {
                if key.kind == KeyStrokeKind::Release {
                    return None;
                }
                if mode.overlay_active() {
                    self.map_overlay_key(key)
                } else if mode.command_session_active() {
                    self.map_command_session_key(key)
                } else {
                    self.map_regular_key(key, mode)
                }
            }
            Event::Paste(text) => {
                if mode.overlay_active() {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(InputOccurrence::Text(trimmed.to_string()))
                    }
                } else if mode.command_session_active() {
                    None
                } else {
                    if self.should_ignore_startup_paste(&text) {
                        return None;
                    }
                    self.editor.insert_str(&text);
                    None
                }
            }
            _ => None,
        }
    }

    fn render(&mut self, mode: &TuiMode) {
        let input = self.editor.buffer().to_string();
        let cursor = self.editor.cursor();
        let mut display_area = Rect::new(0, 0, 0, 0);
        if let Ok(size) = self.tui.size() {
            let width = size.width.max(1);
            mode.set_display_column_width(width as usize);
            display_area = Rect::new(0, 0, size.width, size.height);
        }

        let task_state = mode.task_layout_state();

        if let Some(mut task_state) = task_state {
            task_state.picker_overlay = self.build_picker_overlay(mode);
            task_state.composer_text = input;
            task_state.composer_cursor = cursor;
            task_state.composer_focused = mode.composer_is_focused();
            let expanded_output_rows = Arc::<[crate::app::TranscriptRow]>::from(
                expand_rows_for_display(&task_state.output_rows, display_area.width),
            );
            self.maybe_insert_task_scrollback(
                &task_state,
                expanded_output_rows.clone(),
                display_area,
            );
            let view = task_state.into_view_projection(expanded_output_rows);
            let _ = self.tui.draw(|frame| {
                render_task_layout(frame, &view);
                let area = frame.area();
                if let Some((patch_preview, scroll_offset)) = mode.pending_patch_overlay() {
                    render_overlay_modal_in_area(
                        frame,
                        area,
                        OverlayModal::PatchApprove {
                            patch_preview,
                            scroll_offset,
                        },
                    );
                } else if let Some((tool_name, input_preview, auto_approve_enabled)) =
                    mode.pending_tool_overlay()
                {
                    render_overlay_modal_in_area(
                        frame,
                        area,
                        OverlayModal::ToolPermission {
                            tool_name,
                            input_preview,
                            auto_approve_enabled,
                        },
                    );
                } else if mode.pending_memory_clear_overlay() {
                    render_overlay_modal_in_area(frame, area, OverlayModal::MemoryClear);
                }
            });
        } else {
            self.inline_scrollback = None;
            let _ = self.tui.draw(|frame| {
                let area = frame.area();
                let input_width = area.width.saturating_sub(2).max(1) as usize;
                let input_rows = input_visual_rows(&input, input_width)
                    .clamp(1, crate::ui::render::MAX_INPUT_PANE_ROWS)
                    as u16;
                let panes = split_three_pane_layout(area, input_rows);
                frame.render_widget(Clear, area);
                let history_lines = mode.history_lines();
                let content_width = history_content_width_for_area(&history_lines, panes.history);
                mode.set_display_column_width(content_width);

                let status = mode.status_line();

                render_status_line(frame, panes.header, &status);
                render_messages(frame, panes.history, &history_lines);
                render_input(frame, panes.input, &input, cursor, true);

                if let Some((patch_preview, scroll_offset)) = mode.pending_patch_overlay() {
                    render_overlay_modal_in_area(
                        frame,
                        area,
                        OverlayModal::PatchApprove {
                            patch_preview,
                            scroll_offset,
                        },
                    );
                } else if let Some((tool_name, input_preview, auto_approve_enabled)) =
                    mode.pending_tool_overlay()
                {
                    render_overlay_modal_in_area(
                        frame,
                        area,
                        OverlayModal::ToolPermission {
                            tool_name,
                            input_preview,
                            auto_approve_enabled,
                        },
                    );
                } else if mode.pending_memory_clear_overlay() {
                    render_overlay_modal_in_area(frame, area, OverlayModal::MemoryClear);
                }
            });
        }
    }

    fn should_quit(&self) -> bool {
        self.quit
    }
}

fn build_inline_scrollback_snapshot(
    state: &crate::app::TaskLayoutState,
    expanded_output_rows: Arc<[crate::app::TranscriptRow]>,
    area: Rect,
) -> InlineScrollbackSnapshot {
    let input_width = area.width.saturating_sub(2).max(1) as usize;
    let desired_input_rows = input_visual_rows(&state.composer_text, input_width).saturating_add(1);
    let input_rows = preferred_four_region_input_rows_for_content(
        area.height,
        desired_input_rows.min(u16::MAX as usize) as u16,
    );
    let available_output = area.height.saturating_sub(1).saturating_sub(input_rows) as usize;
    let total = expanded_output_rows.len();
    let max_offset = total.saturating_sub(available_output);
    let offset = state.output_scroll_offset.min(max_offset);
    let visible_start = if available_output == 0 || total == 0 {
        0
    } else {
        total.saturating_sub(available_output.saturating_add(offset))
    };

    InlineScrollbackSnapshot {
        task_id: state.task_id.clone(),
        area,
        visible_start,
        expanded_output_rows,
    }
}

fn inline_rows_to_insert(
    previous: &InlineScrollbackSnapshot,
    current: &InlineScrollbackSnapshot,
) -> Option<(usize, usize)> {
    if previous.task_id != current.task_id || previous.area != current.area {
        return None;
    }

    if current.visible_start <= previous.visible_start {
        return None;
    }

    // Inline scrollback is only safe when newly visible rows are a strict
    // append-only extension of the previous frame. Streaming text can still
    // reflow or mutate earlier wrapped rows, and replaying those stale rows
    // into scrollback duplicates or drops visible content.
    if !transcript_rows_extend_append_only(
        &previous.expanded_output_rows,
        &current.expanded_output_rows,
    ) {
        return None;
    }

    let end = current
        .visible_start
        .min(previous.expanded_output_rows.len());
    (end > previous.visible_start).then_some((previous.visible_start, end))
}

fn transcript_rows_extend_append_only(
    previous: &[crate::app::TranscriptRow],
    current: &[crate::app::TranscriptRow],
) -> bool {
    current.len() >= previous.len()
        && previous
            .iter()
            .zip(current.iter())
            .all(|(prev, next)| prev == next)
}

pub(crate) mod picker;
pub use self::picker::*;

fn task_shortcut_occurrence(key: KeyStroke) -> Option<InputOccurrence> {
    match key.code {
        KeyCode::Char(ch)
            if ch.eq_ignore_ascii_case(&'f')
                && key.modifiers.contains(KeyModifiers::ALT)
                && !key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            Some(InputOccurrence::Text("/fork".to_string()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alt_f_maps_to_fork_command() {
        let occurrence =
            task_shortcut_occurrence(KeyStroke::new(KeyCode::Char('f'), KeyModifiers::ALT));

        assert!(matches!(
            occurrence,
            Some(InputOccurrence::Text(text)) if text == "/fork"
        ));
    }

    #[test]
    fn ctrl_alt_f_does_not_trigger_fork_command() {
        let occurrence = task_shortcut_occurrence(KeyStroke::new(
            KeyCode::Char('f'),
            KeyModifiers::ALT | KeyModifiers::CONTROL,
        ));

        assert!(occurrence.is_none());
    }

    #[test]
    fn inline_scrollback_inserts_rows_that_just_left_the_viewport() {
        let previous = InlineScrollbackSnapshot {
            task_id: "task-1".to_string(),
            area: Rect::new(0, 0, 80, 20),
            visible_start: 2,
            expanded_output_rows: vec![
                crate::app::TranscriptRow::Plain("a".to_string()),
                crate::app::TranscriptRow::Plain("b".to_string()),
                crate::app::TranscriptRow::Plain("c".to_string()),
                crate::app::TranscriptRow::Plain("d".to_string()),
                crate::app::TranscriptRow::Plain("e".to_string()),
            ]
            .into(),
        };
        let current = InlineScrollbackSnapshot {
            task_id: "task-1".to_string(),
            area: Rect::new(0, 0, 80, 20),
            visible_start: 4,
            expanded_output_rows: vec![
                crate::app::TranscriptRow::Plain("a".to_string()),
                crate::app::TranscriptRow::Plain("b".to_string()),
                crate::app::TranscriptRow::Plain("c".to_string()),
                crate::app::TranscriptRow::Plain("d".to_string()),
                crate::app::TranscriptRow::Plain("e".to_string()),
                crate::app::TranscriptRow::Plain("f".to_string()),
            ]
            .into(),
        };

        assert_eq!(inline_rows_to_insert(&previous, &current), Some((2, 4)));
    }

    #[test]
    fn inline_scrollback_skips_resize_and_non_advancing_windows() {
        let previous = InlineScrollbackSnapshot {
            task_id: "task-1".to_string(),
            area: Rect::new(0, 0, 80, 20),
            visible_start: 3,
            expanded_output_rows: vec![crate::app::TranscriptRow::Plain("a".to_string())].into(),
        };
        let resized = InlineScrollbackSnapshot {
            task_id: "task-1".to_string(),
            area: Rect::new(0, 0, 100, 20),
            visible_start: 4,
            expanded_output_rows: vec![].into(),
        };
        let same_window = InlineScrollbackSnapshot {
            task_id: "task-1".to_string(),
            area: Rect::new(0, 0, 80, 20),
            visible_start: 3,
            expanded_output_rows: vec![].into(),
        };

        assert_eq!(inline_rows_to_insert(&previous, &resized), None);
        assert_eq!(inline_rows_to_insert(&previous, &same_window), None);
    }

    #[test]
    fn inline_scrollback_skips_mutated_streaming_windows() {
        let previous = InlineScrollbackSnapshot {
            task_id: "task-1".to_string(),
            area: Rect::new(0, 0, 80, 20),
            visible_start: 1,
            expanded_output_rows: vec![
                crate::app::TranscriptRow::Plain("prompt".to_string()),
                crate::app::TranscriptRow::AssistantText {
                    text: "partial line".to_string(),
                    streaming: true,
                },
                crate::app::TranscriptRow::AssistantText {
                    text: "stable tail".to_string(),
                    streaming: true,
                },
            ]
            .into(),
        };
        let current = InlineScrollbackSnapshot {
            task_id: "task-1".to_string(),
            area: Rect::new(0, 0, 80, 20),
            visible_start: 2,
            expanded_output_rows: vec![
                crate::app::TranscriptRow::Plain("prompt".to_string()),
                crate::app::TranscriptRow::AssistantText {
                    text: "partial line extended".to_string(),
                    streaming: true,
                },
                crate::app::TranscriptRow::AssistantText {
                    text: "stable tail".to_string(),
                    streaming: true,
                },
                crate::app::TranscriptRow::AssistantText {
                    text: "new tail".to_string(),
                    streaming: true,
                },
            ]
            .into(),
        };

        assert_eq!(inline_rows_to_insert(&previous, &current), None);
    }
}
