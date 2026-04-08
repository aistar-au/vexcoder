use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    backend::Backend,
    text::Text,
    widgets::{Clear, Paragraph, Widget},
};
use std::ops::Range;
use std::time::{Duration, Instant};

use crate::app::{
    FileMentionPickerState, PickerOverlayLine, SlashPickerMatch, SlashPickerState, TranscriptRow,
    TuiMode,
};
use crate::runtime::frontend::{FrontendAdapter, ScrollAction, ScrollTarget, UserInputEvent};
use crate::runtime::mode::RuntimeMode;
use crate::startup::{
    looks_like_session_output, should_ignore_startup_paste_text, STARTUP_NOISE_GUARD,
};
use crate::ui::editor::{file_mention_range, InputAction, InputEditor};
use crate::ui::layout::split_three_pane_layout;
use crate::ui::render::{
    history_content_width_for_area, input_visual_rows, render_input, render_messages,
    render_overlay_modal_in_area, render_status_line, render_task_layout, OverlayModal,
};

pub struct ManagedTuiFrontend {
    tui: crate::tui_handle::TuiHandle,
    history_sink: HostScrollbackSink,
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
}

impl ManagedTuiFrontend {
    pub fn new() -> Result<Self> {
        let tui = crate::tui_handle::setup()?;
        Self::drain_startup_events();
        Ok(Self {
            tui,
            history_sink: HostScrollbackSink::default(),
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
        })
    }

    fn current_file_picker(&mut self, mode: &TuiMode) -> Option<FileMentionPickerState> {
        let input = self.editor.buffer();
        let cursor = self.editor.cursor();
        if file_picker_is_dismissed(self.dismissed_file_picker.as_ref(), input, cursor) {
            return None;
        }
        if let Some((cached_input, cached_cursor, cached_picker)) = &self.cached_file_picker {
            if cached_input == input && *cached_cursor == cursor {
                return cached_picker.clone();
            }
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
        if let Some((cached_input, cached_cursor, cached_picker)) = &self.cached_slash_picker {
            if cached_input == input && *cached_cursor == cursor {
                return cached_picker.clone();
            }
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

    fn drain_startup_events() {
        for _ in 0..1024 {
            match event::poll(Duration::from_millis(0)) {
                Ok(true) => {
                    if event::read().is_err() {
                        break;
                    }
                }
                Ok(false) | Err(_) => break,
            }
        }
    }

    /// Returns true when the startup paste filter is active (default: on).
    /// Set `VEX_DISABLE_STARTUP_FILTER=1` to disable for observability.
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

    fn map_editor_action(&mut self, action: InputAction) -> Option<UserInputEvent> {
        match action {
            InputAction::None => None,
            InputAction::Interrupt => Some(UserInputEvent::Interrupt),
            InputAction::Quit => {
                self.quit = true;
                None
            }
            InputAction::Submit(value) => {
                if self.should_ignore_startup_submission(&value) {
                    None
                } else {
                    Some(UserInputEvent::Text(value))
                }
            }
        }
    }

    fn map_overlay_key(&mut self, key: KeyEvent) -> Option<UserInputEvent> {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Interrupt)
            }
            KeyCode::Up => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::LineUp,
            }),
            KeyCode::Down => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::LineDown,
            }),
            KeyCode::PageUp => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::PageUp(10),
            }),
            KeyCode::PageDown => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::PageDown(10),
            }),
            KeyCode::Home => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::Home,
            }),
            KeyCode::End => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Overlay,
                action: ScrollAction::End,
            }),
            KeyCode::Esc => Some(UserInputEvent::Text("esc".to_string())),
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                Some(UserInputEvent::Text(ch.to_string()))
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

    fn map_regular_key(&mut self, key: KeyEvent, mode: &TuiMode) -> Option<UserInputEvent> {
        if key.modifiers.is_empty() {
            // Slash command picker (triggered by `/` prefix).
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
                        // Any other key clears dismiss state so the picker
                        // reopens as the user keeps typing.
                        self.dismissed_slash_picker = false;
                    }
                }
            } else {
                // Reset dismiss state when no longer on a slash prefix.
                if slash_prefix_token(self.editor.buffer()).is_none() {
                    self.dismissed_slash_picker = false;
                }
            }

            // File mention picker (triggered by `@` prefix).
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

        match key.code {
            // Timeline navigation: Alt+Up / Alt+Down, plus Alt+Home / Alt+End
            // to jump directly to the first step or return to live follow mode.
            KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::Timeline,
                    action: ScrollAction::LineUp,
                })
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::Timeline,
                    action: ScrollAction::LineDown,
                })
            }
            KeyCode::Home if key.modifiers.contains(KeyModifiers::ALT) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::Timeline,
                    action: ScrollAction::Home,
                })
            }
            KeyCode::End if key.modifiers.contains(KeyModifiers::ALT) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::Timeline,
                    action: ScrollAction::End,
                })
            }
            // Tab / Shift+Tab also navigate the timeline.
            KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::Timeline,
                    action: ScrollAction::LineUp,
                })
            }
            KeyCode::Tab => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Timeline,
                action: ScrollAction::LineDown,
            }),
            KeyCode::PageUp => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Output,
                action: ScrollAction::PageUp(10),
            }),
            KeyCode::PageDown => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Output,
                action: ScrollAction::PageDown(10),
            }),
            KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::Output,
                    action: ScrollAction::LineUp,
                })
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::Output,
                    action: ScrollAction::LineDown,
                })
            }
            KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::Output,
                    action: ScrollAction::Home,
                })
            }
            KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
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
            KeyCode::Up if key.modifiers.is_empty() => {
                if self
                    .editor
                    .move_cursor_visual_up(self.editor_visual_width())
                {
                    None
                } else {
                    let action = self.editor.apply_key(key);
                    self.map_editor_action(action)
                }
            }
            KeyCode::Down if key.modifiers.is_empty() => {
                if self
                    .editor
                    .move_cursor_visual_down(self.editor_visual_width())
                {
                    None
                } else {
                    let action = self.editor.apply_key(key);
                    self.map_editor_action(action)
                }
            }
            _ => {
                let action = self.editor.apply_key(key);
                self.map_editor_action(action)
            }
        }
    }

    fn map_command_session_key(&mut self, key: KeyEvent) -> Option<UserInputEvent> {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Interrupt)
            }
            KeyCode::PageUp => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Output,
                action: ScrollAction::PageUp(10),
            }),
            KeyCode::PageDown => Some(UserInputEvent::Scroll {
                target: ScrollTarget::Output,
                action: ScrollAction::PageDown(10),
            }),
            KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::Output,
                    action: ScrollAction::Home,
                })
            }
            KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::Output,
                    action: ScrollAction::End,
                })
            }
            KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::Output,
                    action: ScrollAction::LineUp,
                })
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(UserInputEvent::Scroll {
                    target: ScrollTarget::Output,
                    action: ScrollAction::LineDown,
                })
            }
            _ => None,
        }
    }

    /// Build the floating picker overlay lines from the currently active picker.
    fn build_picker_overlay(&mut self, mode: &TuiMode) -> Vec<PickerOverlayLine> {
        // Slash picker takes priority.
        if let Some(picker) = self.current_slash_picker(mode) {
            if picker.matches.is_empty() {
                return Vec::new();
            }
            return build_slash_overlay(&picker.matches, self.selected_slash_hint);
        }

        // File mention picker.
        if let Some(picker) = self.current_file_picker(mode) {
            return build_file_overlay(&picker.prefix, &picker.matches, self.selected_file_hint);
        }

        Vec::new()
    }
}

const HISTORY_INSERT_CHUNK_ROWS: usize = 256;

#[derive(Default)]
struct HostScrollbackSink {
    flushed_committed_rows: usize,
}

impl HostScrollbackSink {
    fn flush<B: Backend>(
        &mut self,
        ratatui_tui: &mut ratatui::Terminal<B>,
        committed_rows: &[TranscriptRow],
        viewport_width: u16,
    ) -> std::io::Result<()> {
        if committed_rows.len() < self.flushed_committed_rows {
            self.flushed_committed_rows = committed_rows.len();
        }

        let pending_rows = &committed_rows[self.flushed_committed_rows..];
        if pending_rows.is_empty() {
            return Ok(());
        }

        let expanded_rows =
            crate::ui::render::expand_rows_for_display(pending_rows, viewport_width.max(1));
        for chunk in expanded_rows.chunks(HISTORY_INSERT_CHUNK_ROWS) {
            let lines = chunk
                .iter()
                .map(crate::ui::render::transcript_output_line)
                .collect::<Vec<_>>();
            ratatui_tui.insert_before(chunk.len() as u16, move |buf| {
                Paragraph::new(Text::from(lines)).render(buf.area, buf);
            })?;
        }

        self.flushed_committed_rows = committed_rows.len();
        Ok(())
    }
}

/// Maximum number of visible entries in the floating picker overlay.
const MAX_PICKER_OVERLAY_VISIBLE: usize = 12;

impl Drop for ManagedTuiFrontend {
    fn drop(&mut self) {
        let _ = crate::tui_handle::restore();
    }
}

impl FrontendAdapter<TuiMode> for ManagedTuiFrontend {
    fn poll_user_input(&mut self, mode: &TuiMode) -> Option<UserInputEvent> {
        if mode.quit_requested() {
            self.quit = true;
            return None;
        }

        // Use a shorter poll timeout during active model turns so streamed
        // tokens flow through the render loop with minimal latency.
        let poll_ms = if mode.is_turn_in_progress() { 1 } else { 16 };
        let Ok(has_event) = event::poll(Duration::from_millis(poll_ms)) else {
            self.quit = true;
            return None;
        };
        if !has_event {
            return None;
        }

        let Ok(ev) = event::read() else {
            self.quit = true;
            return None;
        };

        match ev {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Release {
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
                        Some(UserInputEvent::Text(trimmed.to_string()))
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
        let size = self.tui.size().unwrap_or_default();
        mode.set_display_column_width(size.width.max(1) as usize);

        if self.tui.uses_inline_viewport() {
            let committed_rows = mode.committed_transcript_rows();
            let _ = self.history_sink.flush(
                self.tui.inner_mut(),
                &committed_rows,
                size.width.max(1),
            );
        }

        let task_state = if self.tui.uses_inline_viewport() {
            mode.host_history_task_layout_state()
        } else {
            mode.task_layout_state()
        };

        if let Some(mut task_state) = task_state {
            task_state.picker_overlay = self.build_picker_overlay(mode);
            task_state.composer_text = input;
            task_state.composer_cursor = cursor;
            task_state.composer_focused = mode.composer_is_focused();
            let view = task_state.into_view_projection();
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

pub(crate) mod picker;
pub use self::picker::*;

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{
        backend::TestBackend, buffer::Buffer, widgets::Paragraph, Terminal, TerminalOptions,
        Viewport,
    };

    fn rendered_lines(buffer: &Buffer) -> Vec<String> {
        buffer
            .content
            .chunks(buffer.area.width as usize)
            .map(|cells| {
                let mut line = String::new();
                for cell in cells {
                    line.push_str(cell.symbol());
                }
                line
            })
            .collect()
    }

    #[test]
    fn host_scrollback_sink_flushes_committed_rows() {
        let backend = TestBackend::new(20, 5);
        let mut tui = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(1),
            },
        )
        .expect("inline viewport");
        let mut sink = HostScrollbackSink::default();
        let committed_rows = vec![
            TranscriptRow::Plain("------ Line 1 ------".to_string()),
            TranscriptRow::Plain("------ Line 2 ------".to_string()),
            TranscriptRow::Plain("------ Line 3 ------".to_string()),
            TranscriptRow::Plain("------ Line 4 ------".to_string()),
            TranscriptRow::Plain("------ Line 5 ------".to_string()),
        ];

        sink.flush(&mut tui, &committed_rows, 20)
            .expect("flush committed rows");
        tui
            .draw(|frame| {
                frame.render_widget(Paragraph::new("[---- Viewport ----]"), frame.area());
            })
            .expect("draw viewport");

        assert_eq!(
            rendered_lines(tui.backend().scrollback()),
            vec!["------ Line 1 ------".to_string()]
        );
        assert_eq!(
            rendered_lines(tui.backend().buffer()),
            vec![
                "------ Line 2 ------".to_string(),
                "------ Line 3 ------".to_string(),
                "------ Line 4 ------".to_string(),
                "------ Line 5 ------".to_string(),
                "[---- Viewport ----]".to_string(),
            ]
        );
    }

    #[test]
    fn host_scrollback_sink_idempotent_on_same_rows() {
        let backend = TestBackend::new(20, 5);
        let mut tui = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(1),
            },
        )
        .expect("inline viewport");
        let mut sink = HostScrollbackSink::default();
        let committed_rows = vec![TranscriptRow::Plain("------ Line 1 ------".to_string())];

        sink.flush(&mut tui, &committed_rows, 20)
            .expect("first flush");
        sink.flush(&mut tui, &committed_rows, 20)
            .expect("second flush");
        tui
            .draw(|frame| {
                frame.render_widget(Paragraph::new("[---- Viewport ----]"), frame.area());
            })
            .expect("draw viewport");

        assert!(rendered_lines(tui.backend().scrollback()).is_empty());
        assert_eq!(
            rendered_lines(tui.backend().buffer()),
            vec![
                "------ Line 1 ------".to_string(),
                "[---- Viewport ----]".to_string(),
                "                    ".to_string(),
                "                    ".to_string(),
                "                    ".to_string(),
            ]
        );
    }
}
