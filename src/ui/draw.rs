//! Direct ANSI draw engine for the task-state control surface.
//!
//! This module bypasses ratatui's widget-buffering model and writes ANSI
//! escape sequences directly to a `Write` sink. The design goals are:
//!
//! 1. **Full-screen ownership** — while a turn is in progress the draw engine
//!    owns the entire terminal; the prompt is never yielded between tool calls.
//! 2. **Text-only ANSI** — every visible pixel is a character cell written via
//!    standard escape sequences. No alternate screen buffer, no intermediate
//!    `Buffer` allocation per frame.
//! 3. **Viewport-managed output** — task output is shown in a fixed viewport
//!    that follows the latest lines, while inspector-style content repaints
//!    when selection or approval state changes.
//! 4. **Minimal redraw** — only dirty regions (status bar, activity strip,
//!    output body, input hint) are rewritten each frame.
//!
//! The public entry point is [`TaskDraw`] which is constructed once per task
//! turn and called from the `FrontendAdapter::render` path when
//! `task_layout_state()` returns `Some`.

use crate::app::{StepLifecycle, TaskLayoutState, TimelineEntry};
use crate::ui::input_metrics::{display_width, truncate_to_display_width};
use std::io::Write;

// ── ANSI escape helpers ─────────────────────────────────────────────

const CSI: &str = "\x1b[";
const RESET: &str = "\x1b[0m";

fn move_to(w: &mut dyn Write, row: u16, col: u16) {
    let _ = write!(w, "{CSI}{};{}H", row + 1, col + 1);
}

fn clear_line(w: &mut dyn Write) {
    let _ = write!(w, "{CSI}2K");
}

fn clear_to_end(w: &mut dyn Write) {
    let _ = write!(w, "{CSI}0J");
}

fn set_fg(w: &mut dyn Write, code: u8) {
    let _ = write!(w, "{CSI}38;5;{code}m");
}

fn set_bold(w: &mut dyn Write) {
    let _ = write!(w, "{CSI}1m");
}

fn set_dim(w: &mut dyn Write) {
    let _ = write!(w, "{CSI}2m");
}

fn reset_style(w: &mut dyn Write) {
    let _ = write!(w, "{RESET}");
}

fn hide_cursor(w: &mut dyn Write) {
    let _ = write!(w, "{CSI}?25l");
}

fn show_cursor(w: &mut dyn Write) {
    let _ = write!(w, "{CSI}?25h");
}

// 256-color palette indices matching the ratatui scheme.
const GREEN: u8 = 2;
const RED: u8 = 1;
const CYAN: u8 = 6;
const YELLOW: u8 = 3;
const MAGENTA: u8 = 5;
const GRAY: u8 = 245;
const DIM_GRAY: u8 = 240;
const WHITE: u8 = 15;

fn lifecycle_color(lifecycle: &StepLifecycle) -> u8 {
    match lifecycle {
        StepLifecycle::Completed => GREEN,
        StepLifecycle::Failed => RED,
        StepLifecycle::Running => CYAN,
        StepLifecycle::AwaitingApproval => YELLOW,
        StepLifecycle::UserInput => DIM_GRAY,
        StepLifecycle::CommandSession => MAGENTA,
    }
}

fn lifecycle_prefix(lifecycle: &StepLifecycle) -> &'static str {
    match lifecycle {
        StepLifecycle::Completed => "[ok]",
        StepLifecycle::Failed => "[!]",
        StepLifecycle::Running => "[->]",
        StepLifecycle::AwaitingApproval => "[?]",
        StepLifecycle::UserInput => ">",
        StepLifecycle::CommandSession => "[$$]",
    }
}

// ── Region geometry ─────────────────────────────────────────────────

/// Fixed-height regions within the terminal.
///
/// ```text
/// row 0      ┌─ status bar ──────────────────┐  (1 row)
/// row 1      ├─ changed files (optional) ─────┤  (0..1 rows)
/// row 1..N   ├─ activity strip ───────────────┤  (ACTIVITY_ROWS rows)
/// row N..M   │  output body (unlimited)       │  (remaining rows)
/// row M      ├─ input hint ───────────────────┤  (INPUT_ROWS rows)
/// row M+I    └────────────────────────────────┘
/// ```
const ACTIVITY_ROWS: usize = 6;
const INPUT_ROWS: usize = 2;

struct Regions {
    cols: u16,
    rows: u16,
    status_row: u16,
    files_row: Option<u16>,
    activity_start: u16,
    output_start: u16,
    output_rows: u16,
    input_start: u16,
}

impl Regions {
    fn compute(cols: u16, rows: u16, has_files: bool) -> Self {
        let status_row = 0;
        let files_row = if has_files { Some(1) } else { None };
        let header_height = if has_files { 2 } else { 1 };
        let activity_start = header_height;
        let activity_height = ACTIVITY_ROWS as u16;
        let input_height = INPUT_ROWS as u16;
        let output_start = activity_start + activity_height;
        let output_rows = rows
            .saturating_sub(header_height)
            .saturating_sub(activity_height)
            .saturating_sub(input_height);
        let input_start = output_start + output_rows;

        Regions {
            cols,
            rows,
            status_row,
            files_row,
            activity_start,
            output_start,
            output_rows,
            input_start,
        }
    }
}

// ── TaskDraw ────────────────────────────────────────────────────────

/// Persistent state for the direct-draw engine.
///
/// Constructed once when a task turn begins. Each call to [`draw`] emits
/// only the ANSI sequences needed to update the terminal from the previous
/// frame. Streaming output uses incremental redraw when it is safe to append,
/// while selection-driven inspector content triggers a full viewport repaint.
pub struct TaskDraw {
    /// Number of output body lines already flushed to the terminal.
    output_lines_flushed: usize,
    /// Whether the previous frame reserved a changed-files row.
    last_has_files: bool,
    /// Last rendered activity strip content (for dirty detection).
    last_activity_hash: u64,
    /// Last rendered changed-files row.
    last_files_hash: u64,
    /// Last rendered output pane content.
    last_output_hash: u64,
    /// Last rendered status line (for dirty detection).
    last_status_hash: u64,
    /// Last rendered input hint (for dirty detection).
    last_input_hash: u64,
    /// Terminal dimensions at last draw.
    last_cols: u16,
    last_rows: u16,
    /// Whether the very first frame has been drawn.
    first_frame_done: bool,
}

impl TaskDraw {
    pub fn new() -> Self {
        Self {
            output_lines_flushed: 0,
            last_has_files: false,
            last_activity_hash: 0,
            last_files_hash: 0,
            last_output_hash: 0,
            last_status_hash: 0,
            last_input_hash: 0,
            last_cols: 0,
            last_rows: 0,
            first_frame_done: false,
        }
    }

    /// Reset for a new turn (keeps terminal state but resets line counters).
    pub fn reset(&mut self) {
        self.output_lines_flushed = 0;
        self.last_has_files = false;
        self.last_activity_hash = 0;
        self.last_files_hash = 0;
        self.last_output_hash = 0;
        self.last_status_hash = 0;
        self.last_input_hash = 0;
        self.first_frame_done = false;
    }

    /// Draw the full task-state control surface.
    ///
    /// The caller must provide the current terminal dimensions. This method
    /// writes ANSI escape sequences directly to `w` and flushes once at the
    /// end. It never allocates an intermediate screen buffer.
    pub fn draw<W: Write>(
        &mut self,
        w: &mut W,
        state: &TaskLayoutState,
        term_cols: u16,
        term_rows: u16,
    ) {
        if term_cols == 0 || term_rows == 0 {
            return;
        }

        let size_changed = term_cols != self.last_cols || term_rows != self.last_rows;
        let has_files = !state.changed_files.is_empty();
        let layout_changed = has_files != self.last_has_files;
        self.last_cols = term_cols;
        self.last_rows = term_rows;
        self.last_has_files = has_files;

        let regions = Regions::compute(term_cols, term_rows, has_files);

        // On first frame or terminal resize: full repaint.
        if !self.first_frame_done || size_changed || layout_changed {
            hide_cursor(w);
            self.draw_full(w, state, &regions);
            self.first_frame_done = true;
            let _ = w.flush();
            return;
        }

        // Incremental update: only redraw dirty regions.
        hide_cursor(w);

        // Status bar.
        let status_hash = simple_hash(&state.status_line);
        if status_hash != self.last_status_hash {
            self.draw_status(w, state, &regions);
            self.last_status_hash = status_hash;
        }

        // Changed files row.
        let files_hash = self.compute_files_hash(state);
        if files_hash != self.last_files_hash {
            if let Some(files_row) = regions.files_row {
                self.draw_files(w, state, files_row, regions.cols);
            }
            self.last_files_hash = files_hash;
        }

        // Activity strip.
        let activity_hash = self.compute_activity_hash(state);
        if activity_hash != self.last_activity_hash {
            self.draw_activity(w, state, &regions);
            self.last_activity_hash = activity_hash;
        }

        // Output body.
        let output_hash = self.compute_output_hash(state);
        if output_hash != self.last_output_hash {
            if self.output_is_append_only(state) {
                self.draw_output_incremental(w, state, &regions);
            } else {
                self.draw_output_full(w, state, &regions);
            }
            self.last_output_hash = output_hash;
        }

        // Input hint.
        let input_hash = simple_hash(&state.input_hint);
        if input_hash != self.last_input_hash {
            self.draw_input(w, state, &regions);
            self.last_input_hash = input_hash;
        }

        // Park cursor on input line.
        move_to(w, regions.input_start, 0);
        show_cursor(w);
        let _ = w.flush();
    }

    // ── Full repaint ────────────────────────────────────────────────

    fn draw_full<W: Write>(&mut self, w: &mut W, state: &TaskLayoutState, regions: &Regions) {
        // Clear screen.
        move_to(w, 0, 0);
        clear_to_end(w);

        self.draw_status(w, state, regions);
        self.last_status_hash = simple_hash(&state.status_line);

        if let Some(files_row) = regions.files_row {
            self.draw_files(w, state, files_row, regions.cols);
        }
        self.last_files_hash = self.compute_files_hash(state);

        self.draw_activity(w, state, regions);
        self.last_activity_hash = self.compute_activity_hash(state);

        self.draw_output_full(w, state, regions);
        self.last_output_hash = self.compute_output_hash(state);

        self.draw_input(w, state, regions);
        self.last_input_hash = simple_hash(&state.input_hint);

        // Park cursor.
        move_to(w, regions.input_start, 0);
        show_cursor(w);
    }

    // ── Status bar ──────────────────────────────────────────────────

    fn draw_status<W: Write>(&self, w: &mut W, state: &TaskLayoutState, regions: &Regions) {
        move_to(w, regions.status_row, 0);
        clear_line(w);
        set_dim(w);
        set_fg(w, DIM_GRAY);
        let truncated = truncate_to_width(&state.status_line, regions.cols as usize);
        let _ = write!(w, "{truncated}");
        reset_style(w);
    }

    // ── Changed files ───────────────────────────────────────────────

    fn draw_files<W: Write>(&self, w: &mut W, state: &TaskLayoutState, row: u16, cols: u16) {
        move_to(w, row, 0);
        clear_line(w);
        if state.changed_files.is_empty() {
            return;
        }
        set_dim(w);
        set_fg(w, GRAY);
        let files_text = format!("files: {}", state.changed_files.join(", "));
        let truncated = truncate_to_width(&files_text, cols as usize);
        let _ = write!(w, "{truncated}");
        reset_style(w);
    }

    // ── Activity strip ──────────────────────────────────────────────

    fn draw_activity<W: Write>(&self, w: &mut W, state: &TaskLayoutState, regions: &Regions) {
        if state.timeline_entries.is_empty() {
            self.draw_activity_rows_fallback(w, state, regions);
            return;
        }

        let max_visible = ACTIVITY_ROWS;
        let total = state.timeline_entries.len();
        let selected = state.selected_step.min(total.saturating_sub(1));

        // Title line.
        move_to(w, regions.activity_start, 0);
        clear_line(w);
        set_bold(w);
        set_fg(w, DIM_GRAY);
        let title = self.activity_title(state);
        if total > max_visible {
            let _ = write!(w, "{title} ({}/{})", selected + 1, total);
        } else {
            let _ = write!(w, "{title}");
        }
        reset_style(w);

        // Visible window.
        let window_start = if selected >= max_visible {
            selected + 1 - max_visible
        } else {
            0
        };

        for slot in 0..max_visible {
            let row = regions.activity_start + 1 + slot as u16;
            if row >= regions.output_start {
                break;
            }
            move_to(w, row, 0);
            clear_line(w);

            let entry_index = window_start + slot;
            if entry_index >= total {
                continue;
            }
            let entry = &state.timeline_entries[entry_index];
            let is_selected = entry_index == selected;
            self.draw_timeline_entry(w, entry, is_selected, regions.cols);
        }
    }

    fn draw_activity_rows_fallback<W: Write>(
        &self,
        w: &mut W,
        state: &TaskLayoutState,
        regions: &Regions,
    ) {
        move_to(w, regions.activity_start, 0);
        clear_line(w);
        set_bold(w);
        set_fg(w, DIM_GRAY);
        let _ = write!(w, "{}", self.activity_title(state));
        reset_style(w);

        for slot in 0..ACTIVITY_ROWS {
            let row = regions.activity_start + 1 + slot as u16;
            if row >= regions.output_start {
                break;
            }
            move_to(w, row, 0);
            clear_line(w);

            let Some(activity_row) = state.activity_rows.get(slot) else {
                continue;
            };
            self.draw_legacy_activity_row(w, activity_row, regions.cols);
        }
    }

    fn draw_timeline_entry<W: Write>(
        &self,
        w: &mut W,
        entry: &TimelineEntry,
        is_selected: bool,
        cols: u16,
    ) {
        let prefix = lifecycle_prefix(&entry.lifecycle);
        let color = lifecycle_color(&entry.lifecycle);

        // Selection indicator.
        if is_selected {
            set_bold(w);
            set_fg(w, WHITE);
            let _ = write!(w, "> ");
        } else {
            set_fg(w, DIM_GRAY);
            let _ = write!(w, "  ");
        }

        // Lifecycle prefix.
        set_bold(w);
        set_fg(w, color);
        let _ = write!(w, "{prefix}");
        reset_style(w);

        // Label.
        let _ = write!(w, " ");
        if is_selected {
            set_bold(w);
            set_fg(w, WHITE);
        } else {
            set_fg(w, color);
        }
        let used = 2 + display_width(prefix) + 1; // selector + prefix + space
        let remaining = (cols as usize).saturating_sub(used);
        let truncated = truncate_to_width(&entry.label, remaining);
        let _ = write!(w, "{truncated}");
        reset_style(w);
    }

    fn draw_legacy_activity_row<W: Write>(&self, w: &mut W, row: &str, cols: u16) {
        if let Some(rest) = row.strip_prefix("[ok]") {
            set_bold(w);
            set_fg(w, GREEN);
            let _ = write!(w, "[ok]");
            reset_style(w);
            set_fg(w, GREEN);
            let truncated = truncate_to_width(rest.trim_start(), (cols as usize).saturating_sub(5));
            let _ = write!(w, " {truncated}");
            reset_style(w);
        } else if let Some(rest) = row.strip_prefix("[!]") {
            set_bold(w);
            set_fg(w, RED);
            let _ = write!(w, "[!]");
            reset_style(w);
            set_fg(w, RED);
            let truncated = truncate_to_width(rest.trim_start(), (cols as usize).saturating_sub(4));
            let _ = write!(w, " {truncated}");
            reset_style(w);
        } else if let Some(rest) = row.strip_prefix("[->]") {
            set_bold(w);
            set_fg(w, CYAN);
            let _ = write!(w, "[->]");
            reset_style(w);
            set_fg(w, CYAN);
            let truncated = truncate_to_width(rest.trim_start(), (cols as usize).saturating_sub(5));
            let _ = write!(w, " {truncated}");
            reset_style(w);
        } else if let Some(rest) = row.strip_prefix("[?]") {
            set_bold(w);
            set_fg(w, YELLOW);
            let _ = write!(w, "[?]");
            reset_style(w);
            set_fg(w, YELLOW);
            let truncated = truncate_to_width(rest.trim_start(), (cols as usize).saturating_sub(4));
            let _ = write!(w, " {truncated}");
            reset_style(w);
        } else if let Some(rest) = row.strip_prefix("> ") {
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let _ = write!(w, "> ");
            reset_style(w);
            set_dim(w);
            set_fg(w, GRAY);
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(2));
            let _ = write!(w, "{truncated}");
            reset_style(w);
        } else {
            set_fg(w, GRAY);
            let truncated = truncate_to_width(row, cols as usize);
            let _ = write!(w, "{truncated}");
            reset_style(w);
        }
    }

    fn activity_title(&self, state: &TaskLayoutState) -> &'static str {
        if state.timeline_entries.is_empty() {
            if state
                .activity_rows
                .first()
                .map(|row| row.starts_with("command:"))
                .unwrap_or(false)
            {
                "Session"
            } else if state
                .activity_rows
                .iter()
                .any(|row| row.starts_with("[->]"))
            {
                "Orchestrating"
            } else {
                "Steps"
            }
        } else if state
            .timeline_entries
            .iter()
            .any(|e| e.lifecycle == StepLifecycle::Running)
        {
            "Orchestrating"
        } else if state
            .timeline_entries
            .iter()
            .any(|e| e.lifecycle == StepLifecycle::CommandSession)
        {
            "Session"
        } else {
            "Steps"
        }
    }

    fn compute_activity_hash(&self, state: &TaskLayoutState) -> u64 {
        if state.timeline_entries.is_empty() {
            let mut h = state.activity_rows.len() as u64;
            for row in &state.activity_rows {
                h = h.wrapping_mul(31).wrapping_add(simple_hash(row));
            }
            return h;
        }

        let mut h: u64 = state.selected_step as u64;
        h = h
            .wrapping_mul(31)
            .wrapping_add(state.timeline_entries.len() as u64);
        for entry in &state.timeline_entries {
            h = h
                .wrapping_mul(31)
                .wrapping_add(entry_lifecycle_id(&entry.lifecycle));
            h = h.wrapping_mul(31).wrapping_add(simple_hash(&entry.label));
        }
        h
    }

    fn compute_files_hash(&self, state: &TaskLayoutState) -> u64 {
        let mut h = state.changed_files.len() as u64;
        for path in &state.changed_files {
            h = h.wrapping_mul(31).wrapping_add(simple_hash(path));
        }
        h
    }

    fn compute_output_hash(&self, state: &TaskLayoutState) -> u64 {
        let mut h = simple_hash(self.output_title(state));
        h = h
            .wrapping_mul(31)
            .wrapping_add(state.output_rows.len() as u64);
        for row in &state.output_rows {
            h = h.wrapping_mul(31).wrapping_add(simple_hash(row));
        }
        h
    }

    fn output_is_append_only(&self, state: &TaskLayoutState) -> bool {
        if state.pending_approval.is_some() {
            return false;
        }

        if state.timeline_entries.is_empty() {
            return true;
        }

        matches!(
            state.timeline_entries.get(state.selected_step),
            Some(entry) if entry.lifecycle == StepLifecycle::UserInput
        )
    }

    fn output_title(&self, state: &TaskLayoutState) -> &'static str {
        if !state.timeline_entries.is_empty() {
            "Inspector"
        } else {
            "Output"
        }
    }

    // ── Output body (append-only) ───────────────────────────────────

    fn draw_output_full<W: Write>(
        &mut self,
        w: &mut W,
        state: &TaskLayoutState,
        regions: &Regions,
    ) {
        let viewport_height = regions.output_rows.saturating_sub(1) as usize;

        move_to(w, regions.output_start, 0);
        clear_line(w);
        set_bold(w);
        set_fg(w, DIM_GRAY);
        let _ = write!(w, "{}", self.output_title(state));
        reset_style(w);

        for vp_offset in 0..viewport_height {
            let row = regions.output_start + 1 + vp_offset as u16;
            move_to(w, row, 0);
            clear_line(w);
        }

        let visible_start = state.output_rows.len().saturating_sub(viewport_height);
        for (vp_offset, line) in state.output_rows.iter().skip(visible_start).enumerate() {
            if vp_offset >= viewport_height {
                break;
            }
            let row = regions.output_start + 1 + vp_offset as u16;
            move_to(w, row, 0);
            set_fg(w, GRAY);
            let truncated = truncate_to_width(line, regions.cols as usize);
            let _ = write!(w, "{truncated}");
            reset_style(w);
        }

        self.output_lines_flushed = state.output_rows.len();
    }

    fn draw_output_incremental<W: Write>(
        &mut self,
        w: &mut W,
        state: &TaskLayoutState,
        regions: &Regions,
    ) {
        let total_output = state.output_rows.len();
        if total_output <= self.output_lines_flushed {
            return;
        }

        // Title row for output pane (only on first output or when pane was just cleared).
        if self.output_lines_flushed == 0 {
            move_to(w, regions.output_start, 0);
            clear_line(w);
            set_bold(w);
            set_fg(w, DIM_GRAY);
            let _ = write!(w, "{}", self.output_title(state));
            reset_style(w);
        }

        // Write new lines. We position within the output viewport area.
        // The output viewport starts at output_start + 1 (after title row).
        let viewport_height = regions.output_rows.saturating_sub(1) as usize; // minus title row

        let new_lines = &state.output_rows[self.output_lines_flushed..];
        for (i, line) in new_lines.iter().enumerate() {
            let line_index = self.output_lines_flushed + i;
            // Compute the row within the viewport. If lines exceed viewport,
            // we scroll the viewport to show the latest lines.
            let visible_start = total_output.saturating_sub(viewport_height);

            if line_index < visible_start {
                continue; // This line has scrolled above the viewport.
            }

            let viewport_offset = line_index - visible_start;
            if viewport_offset >= viewport_height {
                continue; // Shouldn't happen, but guard.
            }

            let row = regions.output_start + 1 + viewport_offset as u16;
            move_to(w, row, 0);
            clear_line(w);
            set_fg(w, GRAY);
            let truncated = truncate_to_width(line, regions.cols as usize);
            let _ = write!(w, "{truncated}");
            reset_style(w);
        }

        // If output count exceeds viewport, we need to redraw the entire visible
        // window since lines shift up. Only do this for the scroll case.
        if total_output > viewport_height && self.output_lines_flushed > 0 {
            let visible_start = total_output - viewport_height;
            // Only need to redraw if some previously-flushed lines are now
            // above the viewport (i.e., they shifted).
            if visible_start > 0 {
                for vp_offset in 0..viewport_height {
                    let src_index = visible_start + vp_offset;
                    if src_index >= total_output {
                        break;
                    }
                    let row = regions.output_start + 1 + vp_offset as u16;
                    move_to(w, row, 0);
                    clear_line(w);
                    set_fg(w, GRAY);
                    let truncated =
                        truncate_to_width(&state.output_rows[src_index], regions.cols as usize);
                    let _ = write!(w, "{truncated}");
                    reset_style(w);
                }
            }
        }

        self.output_lines_flushed = total_output;
    }

    // ── Input hint ──────────────────────────────────────────────────

    fn draw_input<W: Write>(&self, w: &mut W, state: &TaskLayoutState, regions: &Regions) {
        for i in 0..INPUT_ROWS as u16 {
            let row = regions.input_start + i;
            if row >= regions.rows {
                break;
            }
            move_to(w, row, 0);
            clear_line(w);
        }

        move_to(w, regions.input_start, 0);

        if let Some(ref approval) = state.pending_approval {
            set_bold(w);
            set_fg(w, YELLOW);
            // First line of approval context.
            let lines: Vec<&str> = approval.lines().collect();
            let first = lines.first().copied().unwrap_or("");
            let truncated = truncate_to_width(first, regions.cols as usize);
            let _ = write!(w, "{truncated}");
            reset_style(w);
            // Second line: prompt.
            if regions.input_start + 1 < regions.rows {
                move_to(w, regions.input_start + 1, 0);
                set_fg(w, YELLOW);
                let _ = write!(w, "[y/n/s] ");
                reset_style(w);
            }
        } else {
            set_fg(w, GRAY);
            // Write up to INPUT_ROWS lines of the input hint.
            let hint_lines: Vec<&str> = state.input_hint.lines().collect();
            for (i, line) in hint_lines.iter().take(INPUT_ROWS).enumerate() {
                if i > 0 {
                    let row = regions.input_start + i as u16;
                    if row >= regions.rows {
                        break;
                    }
                    move_to(w, row, 0);
                }
                let truncated = truncate_to_width(line, regions.cols as usize);
                let _ = write!(w, "{truncated}");
            }
            reset_style(w);
        }
    }
}

impl Default for TaskDraw {
    fn default() -> Self {
        Self::new()
    }
}

// ── Utilities ───────────────────────────────────────────────────────

fn truncate_to_width(text: &str, max_width: usize) -> String {
    if display_width(text) <= max_width {
        return text.to_string();
    }
    truncate_to_display_width(text, max_width)
}

fn simple_hash(s: &str) -> u64 {
    let mut h: u64 = 0;
    for b in s.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u64);
    }
    h
}

fn entry_lifecycle_id(lifecycle: &StepLifecycle) -> u64 {
    match lifecycle {
        StepLifecycle::Completed => 1,
        StepLifecycle::Failed => 2,
        StepLifecycle::Running => 3,
        StepLifecycle::AwaitingApproval => 4,
        StepLifecycle::UserInput => 5,
        StepLifecycle::CommandSession => 6,
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{StepLifecycle, TaskLayoutState, TimelineEntry};

    fn make_state(entries: Vec<TimelineEntry>, output: Vec<&str>) -> TaskLayoutState {
        TaskLayoutState {
            task_id: "test-001".into(),
            status_line: "mode:streaming approval:none".into(),
            activity_rows: vec![],
            timeline_entries: entries,
            selected_step: 0,
            total_steps: 0,
            output_rows: output.into_iter().map(|s| s.to_string()).collect(),
            pending_approval: None,
            input_hint: "> ".into(),
            changed_files: vec![],
        }
    }

    #[test]
    fn first_draw_writes_full_screen() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = make_state(
            vec![TimelineEntry {
                lifecycle: StepLifecycle::Running,
                label: "read_file: running...".into(),
                detail: String::new(),
            }],
            vec!["line 1", "line 2"],
        );

        draw.draw(&mut buf, &state, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        // Must contain ANSI escape sequences.
        assert!(output.contains("\x1b["), "output must contain ANSI escapes");
        // Must contain the status line text.
        assert!(
            output.contains("mode:streaming"),
            "status line must be drawn"
        );
        // Must contain the timeline entry.
        assert!(
            output.contains("read_file: running"),
            "timeline entry must be drawn"
        );
        // Must contain output rows.
        assert!(output.contains("line 1"), "output row 1 must be drawn");
        assert!(output.contains("line 2"), "output row 2 must be drawn");
    }

    #[test]
    fn incremental_draw_skips_unchanged_regions() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = make_state(
            vec![TimelineEntry {
                lifecycle: StepLifecycle::Completed,
                label: "read_file: ok".into(),
                detail: String::new(),
            }],
            vec!["output line"],
        );

        // First draw: full.
        draw.draw(&mut buf, &state, 80, 24);
        let first_len = buf.len();

        // Second draw with identical state: should write much less.
        buf.clear();
        draw.draw(&mut buf, &state, 80, 24);
        let second_len = buf.len();

        assert!(
            second_len < first_len,
            "incremental draw ({second_len} bytes) must be smaller than full draw ({first_len} bytes)"
        );
    }

    #[test]
    fn append_only_output_draws_new_lines() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();

        let state1 = make_state(vec![], vec!["line 1"]);
        draw.draw(&mut buf, &state1, 80, 24);

        buf.clear();
        let state2 = make_state(vec![], vec!["line 1", "line 2", "line 3"]);
        draw.draw(&mut buf, &state2, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        assert!(output.contains("line 2"), "new line 2 must be drawn");
        assert!(output.contains("line 3"), "new line 3 must be drawn");
    }

    #[test]
    fn zero_terminal_size_is_noop() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = make_state(vec![], vec!["text"]);
        draw.draw(&mut buf, &state, 0, 0);
        assert!(buf.is_empty(), "zero-size terminal must produce no output");
    }

    #[test]
    fn resize_triggers_full_repaint() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = make_state(vec![], vec!["text"]);

        draw.draw(&mut buf, &state, 80, 24);
        let first_len = buf.len();

        buf.clear();
        // Same state, different size -> full repaint.
        draw.draw(&mut buf, &state, 120, 30);
        let resize_len = buf.len();

        assert!(
            resize_len >= first_len / 2,
            "resize must trigger a substantial repaint"
        );
    }

    #[test]
    fn truncate_to_width_handles_multibyte() {
        let text = "hello";
        assert_eq!(truncate_to_width(text, 3), "hel");
        assert_eq!(truncate_to_width(text, 100), "hello");

        let wide = "界界a";
        assert_eq!(truncate_to_width(wide, 4), "界界");
        assert_eq!(truncate_to_width(wide, 5), "界界a");
    }

    #[test]
    fn lifecycle_prefixes_have_no_trailing_spaces() {
        // Verify the PR #115 spacing fix is baked into the direct-draw engine.
        let lifecycles = [
            StepLifecycle::Completed,
            StepLifecycle::Failed,
            StepLifecycle::Running,
            StepLifecycle::AwaitingApproval,
            StepLifecycle::UserInput,
            StepLifecycle::CommandSession,
        ];
        for lc in &lifecycles {
            let prefix = lifecycle_prefix(lc);
            assert!(
                !prefix.ends_with(' '),
                "prefix for {lc:?} must not end with a space: got {prefix:?}"
            );
        }
    }

    #[test]
    fn lifecycle_only_changes_redraw_activity() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();

        let running = make_state(
            vec![TimelineEntry {
                lifecycle: StepLifecycle::Running,
                label: "tool: status".into(),
                detail: String::new(),
            }],
            vec![],
        );
        draw.draw(&mut buf, &running, 80, 24);

        buf.clear();
        let completed = make_state(
            vec![TimelineEntry {
                lifecycle: StepLifecycle::Completed,
                label: "tool: status".into(),
                detail: String::new(),
            }],
            vec![],
        );
        draw.draw(&mut buf, &completed, 80, 24);

        let output = String::from_utf8_lossy(&buf);
        assert!(
            output.contains("[ok]"),
            "lifecycle changes must redraw the activity row"
        );
    }

    #[test]
    fn changing_selected_inspector_entry_redraws_output() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();

        let first = TaskLayoutState {
            task_id: "test-001".into(),
            status_line: "mode:streaming approval:none".into(),
            activity_rows: vec![],
            timeline_entries: vec![
                TimelineEntry {
                    lifecycle: StepLifecycle::Completed,
                    label: "read_file: ok".into(),
                    detail: "Tool: read_file\nOutcome: ok".into(),
                },
                TimelineEntry {
                    lifecycle: StepLifecycle::Completed,
                    label: "check: ok".into(),
                    detail: "Tool: check\nOutcome: ok".into(),
                },
            ],
            selected_step: 0,
            total_steps: 2,
            output_rows: vec!["Tool: read_file".into(), "Outcome: ok".into()],
            pending_approval: None,
            input_hint: "> ".into(),
            changed_files: vec![],
        };
        draw.draw(&mut buf, &first, 80, 24);

        buf.clear();
        let second = TaskLayoutState {
            selected_step: 1,
            output_rows: vec!["Tool: check".into(), "Outcome: ok".into()],
            ..first
        };
        draw.draw(&mut buf, &second, 80, 24);

        let output = String::from_utf8_lossy(&buf);
        assert!(
            output.contains("Tool: check"),
            "inspector changes must redraw output rows"
        );
    }

    #[test]
    fn fallback_activity_rows_render_without_timeline_entries() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = TaskLayoutState {
            task_id: "test-001".into(),
            status_line: "mode:streaming approval:none".into(),
            activity_rows: vec!["[->] validate: running...".into(), "> ship it".into()],
            timeline_entries: vec![],
            selected_step: 0,
            total_steps: 0,
            output_rows: vec!["line 1".into()],
            pending_approval: None,
            input_hint: "> ".into(),
            changed_files: vec![],
        };

        draw.draw(&mut buf, &state, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        assert!(output.contains("Orchestrating"));
        assert!(output.contains("validate: running"));
        assert!(output.contains("ship it"));
    }
}
