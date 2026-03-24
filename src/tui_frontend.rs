use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::Clear;
use std::ops::Range;
use std::time::{Duration, Instant};

use crate::app::{
    FileMentionPickerState, PickerOverlayLine, SlashPickerMatch, SlashPickerState, TuiMode,
};
use crate::runtime::frontend::{FrontendAdapter, ScrollAction, ScrollTarget, UserInputEvent};
use crate::startup::{
    looks_like_terminal_transcript, should_ignore_startup_paste_text, STARTUP_NOISE_GUARD,
};
use crate::ui::draw::TaskDraw;
use crate::ui::editor::{file_mention_range, InputAction, InputEditor};
use crate::ui::layout::split_three_pane_layout;
use crate::ui::render::{
    history_content_width_for_area, input_visual_rows, render_input, render_messages,
    render_overlay_modal_in_area, render_status_line, OverlayModal,
};

pub struct ManagedTuiFrontend {
    terminal: crate::terminal::TerminalType,
    quit: bool,
    editor: InputEditor,
    started_at: Instant,
    /// Direct ANSI draw engine for the task-state control surface.
    task_draw: TaskDraw,
    last_hint_input: String,
    last_hint_cursor: usize,
    last_hint_text: String,
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
        let terminal = crate::terminal::setup()?;
        Self::drain_startup_events();
        Ok(Self {
            terminal,
            quit: false,
            editor: InputEditor::new(),
            started_at: Instant::now(),
            task_draw: TaskDraw::new(),
            last_hint_input: String::new(),
            last_hint_cursor: 0,
            last_hint_text: String::new(),
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
        self.last_hint_input.clear();
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
        self.last_hint_input.clear();
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

    fn should_ignore_startup_paste(&self, text: &str) -> bool {
        should_ignore_startup_paste_text(text, self.started_at.elapsed() <= STARTUP_NOISE_GUARD)
    }

    fn should_ignore_startup_submission(&self, text: &str) -> bool {
        self.started_at.elapsed() <= STARTUP_NOISE_GUARD && looks_like_terminal_transcript(text)
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
        self.terminal
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
                        self.last_hint_input.clear();
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
                        self.last_hint_input.clear();
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
            // Timeline navigation: Alt+Up / Alt+Down.
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

    fn prompt_hint(&mut self, mode: &TuiMode, input: &str, cursor: usize) -> String {
        // Slash command picker takes priority when active.
        if let Some(picker) = self.current_slash_picker(mode) {
            if self.last_slash_picker_prefix != picker.prefix {
                self.last_slash_picker_prefix = picker.prefix.clone();
                self.selected_slash_hint = 0;
            }
            let selected = self
                .selected_slash_hint
                .min(picker.matches.len().saturating_sub(1));
            self.last_hint_input = input.to_string();
            self.last_hint_text = render_slash_picker_hint(&picker.matches, selected);
            return self.last_hint_text.clone();
        }

        self.last_slash_picker_prefix.clear();
        self.selected_slash_hint = 0;

        if let Some(picker) = self.current_file_picker(mode) {
            if self.last_file_picker_prefix != picker.prefix {
                self.last_file_picker_prefix = picker.prefix.clone();
                self.selected_file_hint = 0;
            }
            let selected = self
                .selected_file_hint
                .min(picker.matches.len().saturating_sub(1));
            self.last_hint_input = input.to_string();
            self.last_hint_text =
                render_file_picker_hint(&picker.prefix, &picker.matches, selected);
            return self.last_hint_text.clone();
        }

        self.last_file_picker_prefix.clear();
        self.selected_file_hint = 0;
        if self.last_hint_input != input || self.last_hint_cursor != cursor {
            self.last_hint_input = input.to_string();
            self.last_hint_cursor = cursor;
            self.last_hint_text = mode.prompt_hint_for_input(input, cursor);
        }
        self.last_hint_text.clone()
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

/// Maximum number of visible entries in the floating picker overlay.
const MAX_PICKER_OVERLAY_VISIBLE: usize = 12;

pub fn build_file_overlay(
    prefix: &str,
    matches: &[String],
    selected: usize,
) -> Vec<PickerOverlayLine> {
    if matches.is_empty() {
        let text = if prefix.is_empty() {
            "[file] type to search files".to_string()
        } else {
            format!("[file] no matches for {prefix}")
        };
        return vec![PickerOverlayLine {
            text,
            selected: false,
        }];
    }

    let mut lines = Vec::new();
    let selected = selected.min(matches.len().saturating_sub(1));
    let window = MAX_PICKER_OVERLAY_VISIBLE.min(matches.len());
    let start = selected
        .saturating_sub(window / 2)
        .min(matches.len().saturating_sub(window));
    let end = (start + window).min(matches.len());

    lines.push(PickerOverlayLine {
        text: format!(
            "[file] {} match(es) — Up/Down to navigate, Enter to select",
            matches.len()
        ),
        selected: false,
    });

    if start > 0 {
        lines.push(PickerOverlayLine {
            text: format!("  [{start} earlier]"),
            selected: false,
        });
    }

    for (offset, path) in matches[start..end].iter().enumerate() {
        let index = start + offset;
        let is_selected = index == selected;
        let marker = if is_selected { ">" } else { " " };
        lines.push(PickerOverlayLine {
            text: format!("{marker} {path}"),
            selected: is_selected,
        });
    }

    if end < matches.len() {
        lines.push(PickerOverlayLine {
            text: format!("  [{} more]", matches.len() - end),
            selected: false,
        });
    }

    lines
}

pub fn build_slash_overlay(
    matches: &[SlashPickerMatch],
    selected: usize,
) -> Vec<PickerOverlayLine> {
    if matches.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let selected = selected.min(matches.len().saturating_sub(1));
    let window = MAX_PICKER_OVERLAY_VISIBLE.min(matches.len());
    let start = selected
        .saturating_sub(window / 2)
        .min(matches.len().saturating_sub(window));
    let end = (start + window).min(matches.len());

    lines.push(PickerOverlayLine {
        text: format!("/ {} command(s)", matches.len()),
        selected: false,
    });

    for (offset, entry) in matches[start..end].iter().enumerate() {
        let index = start + offset;
        let is_selected = index == selected;
        let marker = if is_selected { ">" } else { " " };
        lines.push(PickerOverlayLine {
            text: format!("{marker} {}", entry.label),
            selected: is_selected,
        });
    }

    lines
}

pub fn active_file_picker(
    mode: &TuiMode,
    buffer: &str,
    cursor: usize,
) -> Option<FileMentionPickerState> {
    let range = file_mention_range(buffer, cursor)?;
    let token = &buffer[range.clone()];
    let prefix = token.strip_prefix('@')?.to_string();
    Some(FileMentionPickerState {
        range,
        prefix: prefix.clone(),
        matches: mode.file_prompt_matches(&prefix),
    })
}

pub fn file_picker_is_dismissed(
    dismissed: Option<&(String, Range<usize>)>,
    input: &str,
    cursor: usize,
) -> bool {
    dismissed.is_some_and(|(dismissed_input, dismissed_range)| {
        dismissed_input == input
            && file_mention_range(input, cursor)
                .as_ref()
                .is_some_and(|current_range| current_range == dismissed_range)
    })
}

pub fn render_file_picker_hint(prefix: &str, matches: &[String], selected: usize) -> String {
    let mut lines = vec!["Prompt".to_string(), "mode: file mention".to_string()];
    if matches.is_empty() {
        if prefix.is_empty() {
            lines.push("[file] no files available".to_string());
        } else {
            lines.push(format!("[file] no matches for {prefix}"));
        }
        return lines.join("\n");
    }

    lines.push(format!("[file] {} match(es)", matches.len()));
    let selected = selected.min(matches.len().saturating_sub(1));
    let window = 12.min(matches.len());
    let start = selected
        .saturating_sub(window / 2)
        .min(matches.len() - window);
    let end = (start + window).min(matches.len());

    if start > 0 {
        lines.push(format!("[file] {start} earlier match(es)"));
    }

    for (offset, path) in matches[start..end].iter().enumerate() {
        let index = start + offset;
        let marker = if index == selected { '>' } else { ' ' };
        lines.push(format!("{marker} [file] {path}"));
    }

    if end < matches.len() {
        lines.push(format!("[file] {} more match(es)", matches.len() - end));
    }
    lines.join("\n")
}

/// Extract the slash prefix token from input (e.g. "/ed" from "/ed something").
/// Returns `None` if the trimmed input does not start with `/`.
pub fn slash_prefix_token(input: &str) -> Option<&str> {
    let trimmed = input.trim_start();
    if !trimmed.starts_with('/') {
        return None;
    }
    Some(trimmed.split_whitespace().next().unwrap_or(trimmed))
}

pub fn active_slash_picker(mode: &TuiMode, buffer: &str) -> Option<SlashPickerState> {
    let token = slash_prefix_token(buffer)?;
    let matches = mode.slash_picker_matches(token);
    if matches.is_empty() {
        return None;
    }
    Some(SlashPickerState {
        prefix: token.to_string(),
        matches,
    })
}

pub fn render_slash_picker_hint(matches: &[SlashPickerMatch], selected: usize) -> String {
    let mut lines = vec!["Prompt".to_string(), "mode: slash".to_string()];
    if matches.is_empty() {
        return lines.join("\n");
    }

    let selected = selected.min(matches.len().saturating_sub(1));
    let window = 12.min(matches.len());
    let start = selected
        .saturating_sub(window / 2)
        .min(matches.len() - window);
    let end = (start + window).min(matches.len());

    if start > 0 {
        lines.push(format!("{start} earlier command(s)"));
    }

    for (offset, entry) in matches[start..end].iter().enumerate() {
        let index = start + offset;
        let marker = if index == selected { '>' } else { ' ' };
        lines.push(format!("{marker} {}", entry.label));
    }

    if end < matches.len() {
        lines.push(format!("{} more command(s)", matches.len() - end));
    }
    lines.join("\n")
}

pub fn apply_slash_picker_selection(editor: &mut InputEditor, command: &str) {
    editor.replace_range(0, editor.buffer().len(), command);
}

pub fn apply_file_picker_selection(editor: &mut InputEditor, range: &Range<usize>, path: &str) {
    let range =
        file_mention_range(editor.buffer(), editor.cursor()).unwrap_or_else(|| range.clone());
    // Directories (trailing /) stay open for drill-down — no trailing space.
    let is_directory = path.ends_with('/');
    let suffix_needs_space = !is_directory
        && editor
            .buffer()
            .get(range.end..)
            .map(|rest| rest.is_empty() || !rest.starts_with(char::is_whitespace))
            .unwrap_or(true);
    let replacement = if suffix_needs_space {
        format!("@{path} ")
    } else {
        format!("@{path}")
    };
    editor.replace_range(range.start, range.end, &replacement);
}

impl Drop for ManagedTuiFrontend {
    fn drop(&mut self) {
        let _ = crate::terminal::restore();
    }
}

impl FrontendAdapter<TuiMode> for ManagedTuiFrontend {
    fn poll_user_input(&mut self, mode: &TuiMode) -> Option<UserInputEvent> {
        if mode.quit_requested() {
            self.quit = true;
            return None;
        }

        let Ok(has_event) = event::poll(Duration::from_millis(16)) else {
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

        if let Some(mut task_state) = mode.task_layout_state() {
            // Direct ANSI draw path — no ratatui buffer allocation.
            // Get terminal size from the ratatui terminal (already tracks it).
            task_state.input_hint = self.prompt_hint(mode, &input, cursor);
            task_state.picker_overlay = self.build_picker_overlay(mode);
            task_state.composer_text = input;
            task_state.composer_cursor = cursor;
            task_state.composer_focused = mode.composer_is_focused();
            let size = self.terminal.size().unwrap_or_default();
            let mut stdout = std::io::stdout();
            self.task_draw
                .draw(&mut stdout, &task_state, size.width, size.height);
        } else {
            // Reset the direct-draw state when leaving task mode so the
            // next turn starts with a clean slate.
            self.task_draw.reset();

            let _ = self.terminal.draw(|frame| {
                let area = frame.area();
                let input_width = area.width.saturating_sub(2).max(1) as usize;
                let input_rows = input_visual_rows(&input, input_width).max(1) as u16;
                let panes = split_three_pane_layout(area, input_rows);
                frame.render_widget(Clear, area);
                let history_width =
                    history_content_width_for_area(mode.history_lines(), panes.history);
                mode.set_history_content_width(history_width);

                let status = mode.status_line();
                let history_scroll = mode.history_scroll_offset();

                render_status_line(frame, panes.header, &status);
                render_messages(frame, panes.history, mode.history_lines(), history_scroll);
                render_input(frame, panes.input, &input, cursor);

                if let Some((patch_preview, scroll_offset)) = mode.pending_patch_overlay() {
                    render_overlay_modal_in_area(
                        frame,
                        area,
                        OverlayModal::PatchApprove {
                            patch_preview,
                            scroll_offset,
                            viewport_rows: panes.history.height.max(1) as usize,
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
                    render_overlay_modal_in_area(
                        frame,
                        area,
                        OverlayModal::ToolPermission {
                            tool_name: "memory clear",
                            input_preview: "clear all notes? type y to confirm, n to cancel",
                            auto_approve_enabled: false,
                        },
                    );
                }
            });
        }
    }

    fn should_quit(&self) -> bool {
        self.quit
    }
}
