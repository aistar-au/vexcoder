//! Adaptive ANSI draw engine for the operator workspace surface.
//!
//! This module writes ANSI escape sequences directly to a `Write` sink,
//! owning the full terminal for the entire session. The design goals are:
//!
//! 1. **Persistent full-screen ownership** — the draw engine owns the
//!    terminal at all times; the prompt is never yielded between tool
//!    calls or after a turn completes.
//! 2. **Flowing transcript** — tool calls, results, and model responses
//!    stream vertically in a continuous log, not a fixed-height window.
//! 3. **Adaptive layout** — the timeline, transcript, and composer areas
//!    scale with the terminal dimensions rather than using fixed row counts.
//! 4. **Human-readable status** — the header shows plain-language state
//!    instead of machine-debug flags.
//! 5. **Minimal redraw** — only dirty regions are rewritten each frame.
//!
//! The public entry point is [`TaskDraw`] which persists across the
//! session and is called from the `FrontendAdapter::render` path.

mod ansi;
mod regions;
mod transcript;

#[cfg(test)]
mod tests;

use crate::app::{OutputScrollAnchor, StepLifecycle, TaskLayoutState, TimelineEntry};
use crate::ui::input_metrics::{
    cursor_row_col, display_width, truncate_to_display_width, visual_window_start, wrap_input_lines,
};
use ansi::*;
use regions::Regions;
use std::io::Write;

// ── TaskDraw ────────────────────────────────────────────────────────

/// Persistent state for the adaptive draw engine.
///
/// Constructed once at session start and called from
/// `FrontendAdapter::render`. Each call to [`draw`] emits only the ANSI
/// sequences needed to update dirty regions from the previous frame.
pub struct TaskDraw {
    /// Number of transcript lines already flushed to the terminal.
    output_lines_flushed: usize,
    /// Whether the previous frame reserved a changed-files row.
    last_has_files: bool,
    /// Last rendered timeline content (for dirty detection).
    last_timeline_hash: u64,
    /// Last rendered changed-files row.
    last_files_hash: u64,
    /// Last rendered transcript content.
    last_transcript_hash: u64,
    /// Last rendered header (for dirty detection).
    last_header_hash: u64,
    /// Last rendered composer (for dirty detection).
    last_composer_hash: u64,
    /// Terminal dimensions at last draw.
    last_cols: u16,
    last_rows: u16,
    /// Whether the very first frame has been drawn.
    first_frame_done: bool,
    /// Monotonic frame counter for spinner animation.
    frame_counter: u64,
    /// Whether the transcript is currently inside a code block.
    in_code_block: bool,
}

impl TaskDraw {
    pub fn new() -> Self {
        Self {
            output_lines_flushed: 0,
            last_has_files: false,
            last_timeline_hash: 0,
            last_files_hash: 0,
            last_transcript_hash: 0,
            last_header_hash: 0,
            last_composer_hash: 0,
            last_cols: 0,
            last_rows: 0,
            first_frame_done: false,
            frame_counter: 0,
            in_code_block: false,
        }
    }

    /// Reset for a new turn (keeps terminal state but resets line counters).
    pub fn reset(&mut self) {
        self.output_lines_flushed = 0;
        self.last_has_files = false;
        self.last_timeline_hash = 0;
        self.last_files_hash = 0;
        self.last_transcript_hash = 0;
        self.last_header_hash = 0;
        self.last_composer_hash = 0;
        self.first_frame_done = false;
        self.in_code_block = false;
    }

    /// Draw the full operator workspace surface.
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

        self.frame_counter = self.frame_counter.wrapping_add(1);
        let size_changed = term_cols != self.last_cols || term_rows != self.last_rows;
        let has_files = !state.changed_files.is_empty();
        let layout_changed = has_files != self.last_has_files;
        self.last_cols = term_cols;
        self.last_rows = term_rows;
        self.last_has_files = has_files;

        let regions = Regions::compute(
            term_cols,
            term_rows,
            has_files,
            state.timeline_entries.len(),
        );

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

        // Header.
        let header_hash = simple_hash(&state.status_line);
        if header_hash != self.last_header_hash {
            self.draw_header(w, state, &regions);
            self.last_header_hash = header_hash;
        }

        // Changed files row.
        let files_hash = self.compute_files_hash(state);
        if files_hash != self.last_files_hash {
            if let Some(files_row) = regions.files_row {
                self.draw_files(w, state, files_row, regions.cols);
            }
            self.last_files_hash = files_hash;
        }

        // Timeline.
        let timeline_hash = self.compute_timeline_hash(state);
        if timeline_hash != self.last_timeline_hash {
            self.draw_timeline(w, state, &regions);
            self.last_timeline_hash = timeline_hash;
        }

        // Transcript.
        let transcript_hash = self.compute_transcript_hash(state);
        if transcript_hash != self.last_transcript_hash {
            if self.transcript_is_append_only(state) {
                self.draw_transcript_incremental(w, state, &regions);
            } else {
                self.draw_transcript_full(w, state, &regions);
            }
            self.last_transcript_hash = transcript_hash;
        }

        // Composer.
        let composer_hash = self.compute_composer_hash(state);
        if composer_hash != self.last_composer_hash {
            self.draw_composer(w, state, &regions);
            self.last_composer_hash = composer_hash;
        }

        // Status bar (always redraw — cheap single-line write).
        self.draw_status_bar(w, state, &regions);

        let (cursor_row, cursor_col) = composer_cursor_position(state, &regions);
        move_to(w, cursor_row, cursor_col);
        show_cursor(w);
        let _ = w.flush();
    }

    // ── Full repaint ────────────────────────────────────────────────

    fn draw_full<W: Write>(&mut self, w: &mut W, state: &TaskLayoutState, regions: &Regions) {
        move_to(w, 0, 0);
        clear_to_end(w);

        self.draw_header(w, state, regions);
        self.last_header_hash = simple_hash(&state.status_line);

        if let Some(files_row) = regions.files_row {
            self.draw_files(w, state, files_row, regions.cols);
        }
        self.last_files_hash = self.compute_files_hash(state);

        self.draw_timeline(w, state, regions);
        self.last_timeline_hash = self.compute_timeline_hash(state);

        self.draw_transcript_full(w, state, regions);
        self.last_transcript_hash = self.compute_transcript_hash(state);

        self.draw_composer(w, state, regions);
        self.last_composer_hash = self.compute_composer_hash(state);

        self.draw_status_bar(w, state, regions);

        let (cursor_row, cursor_col) = composer_cursor_position(state, regions);
        move_to(w, cursor_row, cursor_col);
        show_cursor(w);
    }

    // ── Header ──────────────────────────────────────────────────────

    fn draw_header<W: Write>(&self, w: &mut W, state: &TaskLayoutState, regions: &Regions) {
        move_to(w, regions.header_row, 0);
        clear_line(w);

        // Parse the status line to extract human-readable components.
        // The status_line format is: "mode:X approval:Y history:N repo:R inst:I tokens:T"
        let parts = parse_status_parts(&state.status_line);

        // Left border accent + repo name — bold white.
        set_fg(w, DIM_GRAY);
        let _ = write!(w, "\u{2502} "); // │
        set_bold(w);
        set_fg(w, YELLOW);
        let _ = write!(w, "\u{2605} "); // ★
        set_fg(w, WHITE);
        let _ = write!(w, "{}", parts.repo);
        reset_style(w);

        // Separator.
        set_fg(w, DIM_GRAY);
        let _ = write!(w, " \u{00b7} ");
        reset_style(w);

        // Mode — color-coded.
        let (mode_label, mode_color) = match parts.mode.as_str() {
            "streaming" => ("running", CYAN),
            "command-session" => ("session", MAGENTA),
            "overlay" => ("approval", YELLOW),
            "cancelling" => ("cancelling", RED),
            "quit-arm" => ("quit?", RED),
            _ => ("ready", GREEN),
        };
        set_bold(w);
        set_fg(w, mode_color);
        let _ = write!(w, "{mode_label}");
        reset_style(w);

        // Changed files count (if any).
        if !state.changed_files.is_empty() {
            set_fg(w, DIM_GRAY);
            let _ = write!(w, " \u{00b7} ");
            reset_style(w);
            set_fg(w, GRAY);
            let _ = write!(
                w,
                "{} file{} changed",
                state.changed_files.len(),
                if state.changed_files.len() == 1 {
                    ""
                } else {
                    "s"
                }
            );
            reset_style(w);
        }

        // Timeline step count (if active).
        if !state.timeline_entries.is_empty() {
            set_fg(w, DIM_GRAY);
            let _ = write!(w, " \u{00b7} ");
            reset_style(w);
            let running = state
                .timeline_entries
                .iter()
                .filter(|e| e.lifecycle == StepLifecycle::Running)
                .count();
            let completed = state
                .timeline_entries
                .iter()
                .filter(|e| e.lifecycle == StepLifecycle::Completed)
                .count();
            if running > 0 {
                // Animated progress indicator for running tasks.
                let idx = (self.frame_counter as usize) % PROGRESS_FRAMES.len();
                set_fg(w, CYAN);
                let _ = write!(w, "{} ", PROGRESS_FRAMES[idx]);
                reset_style(w);
                set_fg(w, GRAY);
                let _ = write!(w, "{running} active");
                if completed > 0 {
                    let _ = write!(w, ", {completed} done");
                }
            } else if completed > 0 {
                set_fg(w, GRAY);
                let _ = write!(
                    w,
                    "{completed} step{} done",
                    if completed == 1 { "" } else { "s" }
                );
            }
            reset_style(w);
        }

        if state.total_steps > 0 {
            set_fg(w, DIM_GRAY);
            let _ = write!(w, " \u{00b7} ");
            reset_style(w);
            set_dim(w);
            set_fg(w, BLUE);
            let _ = write!(w, "step {}/{}", state.selected_step + 1, state.total_steps);
            reset_style(w);
        }

        // Context-window token counter — shown once at least one turn has
        // completed and session tokens have been recorded.  Expressed as a
        // compact "~1.2k ctx" indicator so the operator can see how much of
        // the model context window has been consumed so far.
        if parts.tokens > 0 {
            set_fg(w, DIM_GRAY);
            let _ = write!(w, " \u{00b7} ");
            reset_style(w);
            set_dim(w);
            set_fg(w, BLUE);
            let _ = write!(w, "~{:.1}k ctx", parts.tokens_k);
            reset_style(w);
        }

        // Instructions path (dimmed, right side info).
        if parts.inst != "none" {
            set_fg(w, DIM_GRAY);
            let _ = write!(w, " \u{00b7} ");
            set_dim(w);
            let _ = write!(w, "{}", parts.inst);
            reset_style(w);
        }
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
        let files_text = format!("  {}", state.changed_files.join("  "));
        let truncated = truncate_to_width(&files_text, cols as usize);
        let _ = write!(w, "{truncated}");
        reset_style(w);
    }

    // ── Timeline (adaptive height) ──────────────────────────────────

    fn draw_timeline<W: Write>(&self, w: &mut W, state: &TaskLayoutState, regions: &Regions) {
        let visible_slots = (regions.timeline_rows.saturating_sub(1)) as usize; // -1 for separator

        if state.timeline_entries.is_empty() {
            self.draw_timeline_fallback(w, state, regions);
            return;
        }

        let total = state.timeline_entries.len();
        let selected = state.selected_step.min(total.saturating_sub(1));

        // If everything fits, no indicators needed.
        if total <= visible_slots {
            for slot in 0..total {
                let row = regions.timeline_start + slot as u16;
                if row >= regions.transcript_start {
                    break;
                }
                move_to(w, row, 0);
                clear_line(w);
                let entry = &state.timeline_entries[slot];
                let is_selected = slot == selected;
                self.draw_timeline_entry(w, entry, is_selected, regions.cols);
            }
            // Clear remaining slots.
            for slot in total..visible_slots {
                let row = regions.timeline_start + slot as u16;
                if row >= regions.transcript_start {
                    break;
                }
                move_to(w, row, 0);
                clear_line(w);
            }
        } else {
            // Scrolling required. Pessimistically reserve 2 indicator slots,
            // then reclaim if only one indicator is actually needed.
            let mut entry_cap = visible_slots.saturating_sub(2);
            let mut window_start = if selected >= entry_cap {
                selected + 1 - entry_cap
            } else {
                0
            };
            if window_start + entry_cap > total {
                window_start = total.saturating_sub(entry_cap);
            }

            let mut show_above = window_start > 0;
            let mut show_below = window_start + entry_cap < total;

            // Reclaim spare slot when only one indicator is needed.
            if !show_above || !show_below {
                entry_cap = visible_slots - show_above as usize - show_below as usize;
                if selected >= window_start + entry_cap {
                    window_start = selected + 1 - entry_cap;
                }
                if window_start + entry_cap > total {
                    window_start = total.saturating_sub(entry_cap);
                }
                show_above = window_start > 0;
                show_below = window_start + entry_cap < total;
            }

            let above_count = window_start;
            let below_count = total.saturating_sub(window_start + entry_cap);

            for slot in 0..visible_slots {
                let row = regions.timeline_start + slot as u16;
                if row >= regions.transcript_start {
                    break;
                }
                move_to(w, row, 0);
                clear_line(w);

                if slot == 0 && show_above {
                    set_dim(w);
                    set_fg(w, DIM_GRAY);
                    let _ = write!(w, "   \u{25b2} {above_count} more above"); // ▲
                    reset_style(w);
                    continue;
                }

                if slot == visible_slots - 1 && show_below {
                    set_dim(w);
                    set_fg(w, DIM_GRAY);
                    let _ = write!(w, "   \u{25bc} {below_count} more below"); // ▼
                    reset_style(w);
                    continue;
                }

                let entry_slot = slot - show_above as usize;
                let entry_index = window_start + entry_slot;
                if entry_index >= total {
                    continue;
                }
                let entry = &state.timeline_entries[entry_index];
                let is_selected = entry_index == selected;
                self.draw_timeline_entry(w, entry, is_selected, regions.cols);
            }
        }

        // Separator line between timeline and transcript — star accent.
        let sep_row = regions.transcript_start.saturating_sub(1);
        if sep_row >= regions.timeline_start {
            draw_labeled_separator(w, sep_row, regions.cols, &state.output_title);
        }
    }

    fn draw_timeline_fallback<W: Write>(
        &self,
        w: &mut W,
        state: &TaskLayoutState,
        regions: &Regions,
    ) {
        let visible_slots = (regions.timeline_rows.saturating_sub(1)) as usize;

        for slot in 0..visible_slots {
            let row = regions.timeline_start + slot as u16;
            if row >= regions.transcript_start {
                break;
            }
            move_to(w, row, 0);
            clear_line(w);

            if let Some(activity_row) = state.activity_rows.get(slot) {
                self.draw_legacy_activity_row(w, activity_row, regions.cols);
            }
        }

        // Separator — star accent.
        let sep_row = regions.transcript_start.saturating_sub(1);
        if sep_row >= regions.timeline_start {
            draw_labeled_separator(w, sep_row, regions.cols, &state.output_title);
        }
    }

    fn draw_timeline_entry<W: Write>(
        &self,
        w: &mut W,
        entry: &TimelineEntry,
        is_selected: bool,
        cols: u16,
    ) {
        // For running entries, use animated spinner instead of static prefix.
        let spinner_buf;
        let prefix = if entry.lifecycle == StepLifecycle::Running {
            let idx = (self.frame_counter as usize) % SPINNER_FRAMES.len();
            spinner_buf = SPINNER_FRAMES[idx];
            spinner_buf
        } else {
            lifecycle_prefix(&entry.lifecycle)
        };
        let color = lifecycle_color(&entry.lifecycle);

        // Selection indicator — star-themed pointer.
        if is_selected {
            set_bold(w);
            set_fg(w, YELLOW);
            let _ = write!(w, " \u{2726} "); // ✦
        } else {
            let _ = write!(w, "   ");
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
            set_fg(w, GRAY);
        }
        let used = 3 + display_width(prefix) + 1; // selector(3) + prefix + space
        let remaining = (cols as usize).saturating_sub(used);
        let truncated = truncate_to_width(&entry.label, remaining);
        let _ = write!(w, "{truncated}");
        reset_style(w);
    }

    fn draw_legacy_activity_row<W: Write>(&self, w: &mut W, row: &str, cols: u16) {
        if let Some(rest) = row.strip_prefix("[ok]") {
            set_bold(w);
            set_fg(w, GREEN);
            let _ = write!(w, "   \u{2605}"); // ★
            reset_style(w);
            set_fg(w, GRAY);
            let truncated = truncate_to_width(rest.trim_start(), (cols as usize).saturating_sub(6));
            let _ = write!(w, " {truncated}");
            reset_style(w);
        } else if let Some(rest) = row.strip_prefix("[!]") {
            set_bold(w);
            set_fg(w, RED);
            let _ = write!(w, "   \u{2716}"); // ✖
            reset_style(w);
            set_fg(w, GRAY);
            let truncated = truncate_to_width(rest.trim_start(), (cols as usize).saturating_sub(6));
            let _ = write!(w, " {truncated}");
            reset_style(w);
        } else if let Some(rest) = row.strip_prefix("[->]") {
            let idx = (self.frame_counter as usize) % SPINNER_FRAMES.len();
            set_bold(w);
            set_fg(w, CYAN);
            let _ = write!(w, "   {}", SPINNER_FRAMES[idx]);
            reset_style(w);
            set_fg(w, GRAY);
            let truncated = truncate_to_width(rest.trim_start(), (cols as usize).saturating_sub(6));
            let _ = write!(w, " {truncated}");
            reset_style(w);
        } else if let Some(rest) = row.strip_prefix("[?]") {
            set_bold(w);
            set_fg(w, YELLOW);
            let _ = write!(w, "   \u{2606}"); // ☆
            reset_style(w);
            set_fg(w, GRAY);
            let truncated = truncate_to_width(rest.trim_start(), (cols as usize).saturating_sub(6));
            let _ = write!(w, " {truncated}");
            reset_style(w);
        } else if let Some(rest) = row.strip_prefix("> ") {
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let _ = write!(w, "   \u{203a} "); // ›
            reset_style(w);
            set_dim(w);
            set_fg(w, GRAY);
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(5));
            let _ = write!(w, "{truncated}");
            reset_style(w);
        } else {
            set_fg(w, GRAY);
            let _ = write!(w, "   ");
            let truncated = truncate_to_width(row, (cols as usize).saturating_sub(3));
            let _ = write!(w, "{truncated}");
            reset_style(w);
        }
    }

    // ── Transcript (flowing) ────────────────────────────────────────

    fn draw_transcript_full<W: Write>(
        &mut self,
        w: &mut W,
        state: &TaskLayoutState,
        regions: &Regions,
    ) {
        let viewport_height = regions.transcript_rows as usize;
        let (visible_start, visible_end) = transcript_window(state, viewport_height);

        // Clear the transcript area.
        for vp_offset in 0..viewport_height {
            let row = regions.transcript_start + vp_offset as u16;
            move_to(w, row, 0);
            clear_line(w);
        }

        // Reset code block state for full redraws.
        self.in_code_block = false;
        // Walk all lines from the start to track code block state correctly,
        // but only render lines in the visible window.
        for (i, line) in state.output_rows.iter().enumerate() {
            if i < visible_start {
                // Track code block state for lines above the viewport.
                if line.starts_with("```") {
                    self.in_code_block = !self.in_code_block;
                }
                continue;
            }
            let vp_offset = i - visible_start;
            if i >= visible_end || vp_offset >= viewport_height {
                break;
            }
            let row = regions.transcript_start + vp_offset as u16;
            move_to(w, row, 0);
            self.draw_transcript_line(w, line, regions.cols);
        }

        // Scroll position indicator — show when content exceeds viewport.
        let total = state.output_rows.len();
        if total > viewport_height && viewport_height > 0 {
            draw_scroll_indicator(
                w,
                regions.transcript_start,
                viewport_height,
                visible_start,
                total,
                regions.cols,
            );
        }

        self.output_lines_flushed = state.output_rows.len();
    }

    fn draw_transcript_incremental<W: Write>(
        &mut self,
        w: &mut W,
        state: &TaskLayoutState,
        regions: &Regions,
    ) {
        let total_output = state.output_rows.len();
        if total_output <= self.output_lines_flushed {
            return;
        }

        let viewport_height = regions.transcript_rows as usize;
        let (visible_start, visible_end) = transcript_window(state, viewport_height);

        // Rebuild code-block state by scanning all lines before the viewport.
        self.in_code_block = false;
        for line in state.output_rows.iter().take(visible_start) {
            if line.starts_with("```") {
                self.in_code_block = !self.in_code_block;
            }
        }

        // Redraw the entire visible window so code-block state is consistent.
        for vp_offset in 0..viewport_height {
            let src_index = visible_start + vp_offset;
            if src_index >= visible_end || src_index >= total_output {
                break;
            }
            let row = regions.transcript_start + vp_offset as u16;
            move_to(w, row, 0);
            clear_line(w);
            self.draw_transcript_line(w, &state.output_rows[src_index], regions.cols);
        }

        self.output_lines_flushed = total_output;
    }

    fn transcript_is_append_only(&self, state: &TaskLayoutState) -> bool {
        if state.output_scroll_anchor != OutputScrollAnchor::Bottom
            || state.output_scroll_offset > 0
        {
            return false;
        }

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

    // ── Composer ────────────────────────────────────────────────────

    fn draw_composer<W: Write>(&self, w: &mut W, state: &TaskLayoutState, regions: &Regions) {
        // Clear composer area.
        for i in 0..regions.composer_rows {
            let row = regions.composer_start + i;
            if row >= regions.rows {
                break;
            }
            move_to(w, row, 0);
            clear_line(w);
        }

        move_to(w, regions.composer_start, 0);

        if let Some(ref approval) = state.pending_approval {
            set_bold(w);
            set_fg(w, YELLOW);
            let _ = write!(w, "Approval");
            reset_style(w);
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let actions = "  y approve  n deny  s approve all";
            let truncated = truncate_to_width(actions, regions.cols.saturating_sub(10) as usize);
            let _ = write!(w, "{truncated}");
            reset_style(w);

            let lines: Vec<&str> = approval.lines().collect();
            let body_width = regions.cols.saturating_sub(2).max(1) as usize;
            for offset in 0..regions.composer_rows.saturating_sub(1) as usize {
                let row = regions.composer_start + 1 + offset as u16;
                if row >= regions.rows {
                    break;
                }
                move_to(w, row, 0);
                set_fg(w, YELLOW);
                let _ = write!(w, "• ");
                reset_style(w);
                if let Some(line) = lines.get(offset).copied().filter(|line| !line.is_empty()) {
                    set_fg(w, GRAY);
                    let truncated = truncate_to_width(line, body_width);
                    let _ = write!(w, "{truncated}");
                    reset_style(w);
                }
            }
        } else {
            let input_width = regions.cols.saturating_sub(2).max(1) as usize;
            let input_lines = wrap_input_lines(&state.composer_text, input_width);
            let (cursor_row, _) =
                cursor_row_col(&state.composer_text, state.composer_cursor, input_width);
            let body_rows = regions.composer_rows.saturating_sub(1).max(1) as usize;
            let window_start = visual_window_start(cursor_row, body_rows);
            let hint_lines: Vec<&str> = state.input_hint.lines().collect();

            set_bold(w);
            set_fg(w, WHITE);
            let _ = write!(w, "Prompt");
            reset_style(w);
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let chrome = "  / command  @ file  ! shell  paste block  Shift+Enter newline";
            let truncated = truncate_to_width(chrome, regions.cols.saturating_sub(8) as usize);
            let _ = write!(w, "{truncated}");
            reset_style(w);

            for offset in 0..body_rows {
                let row = regions.composer_start + 1 + offset as u16;
                if row >= regions.rows {
                    break;
                }
                move_to(w, row, 0);

                set_fg(w, CYAN);
                let _ = write!(w, "{}", if offset == 0 { "› " } else { "  " });
                reset_style(w);

                let line_index = window_start + offset;
                if let Some(line) = input_lines.get(line_index).filter(|line| !line.is_empty()) {
                    set_fg(w, WHITE);
                    let truncated = truncate_to_width(line, input_width);
                    let _ = write!(w, "{truncated}");
                    reset_style(w);
                    continue;
                }

                let hint = hint_lines
                    .get(line_index + 1)
                    .copied()
                    .or_else(|| hint_lines.get(1).copied())
                    .filter(|line| !line.is_empty());
                if let Some(hint) = hint {
                    set_fg(w, DIM_GRAY);
                    let truncated = truncate_to_width(hint, input_width);
                    let _ = write!(w, "{truncated}");
                    reset_style(w);
                }
            }
        }
    }

    // ── Status bar ──────────────────────────────────────────────────

    fn draw_status_bar<W: Write>(&self, w: &mut W, state: &TaskLayoutState, regions: &Regions) {
        move_to(w, regions.status_bar_row, 0);
        clear_line(w);

        // Background: full-width dim bar.
        set_dim(w);
        set_fg(w, DIM_GRAY);

        // Left side: key hints.
        let is_approval = state.pending_approval.is_some();
        let hints = if is_approval {
            " y approve  n deny  s approve all"
        } else {
            " PgUp/PgDn transcript  Alt+\u{2191}/\u{2193} steps  Shift+Enter newline  Enter submit"
        };
        let _ = write!(w, "{hints}");

        // Right side: task ID (right-aligned).
        if !state.task_id.is_empty() {
            let scroll_state = if state.output_scroll_offset > 0 {
                match state.output_scroll_anchor {
                    OutputScrollAnchor::Bottom => {
                        format!("scroll:+{}  ", state.output_scroll_offset)
                    }
                    OutputScrollAnchor::Top => {
                        format!("detail:{}  ", state.output_scroll_offset + 1)
                    }
                }
            } else {
                String::new()
            };
            let right_text = format!("{scroll_state}task:{} ", state.task_id);
            let right_len = display_width(&right_text);
            let left_len = display_width(hints);
            let gap = (regions.cols as usize).saturating_sub(left_len + right_len);
            for _ in 0..gap {
                let _ = write!(w, " ");
            }
            let _ = write!(w, "{right_text}");
        }

        reset_style(w);
    }

    // ── Hash computation ────────────────────────────────────────────

    fn compute_timeline_hash(&self, state: &TaskLayoutState) -> u64 {
        let has_running = state
            .timeline_entries
            .iter()
            .any(|e| e.lifecycle == StepLifecycle::Running);
        // Legacy activity rows with [->] also animate.
        let has_running_legacy = state.activity_rows.iter().any(|r| r.starts_with("[->]"));

        if state.timeline_entries.is_empty() {
            let mut h = state.activity_rows.len() as u64;
            for row in &state.activity_rows {
                h = h.wrapping_mul(31).wrapping_add(simple_hash(row));
            }
            // Include frame counter when running to force spinner redraws.
            if has_running_legacy {
                h = h.wrapping_mul(31).wrapping_add(self.frame_counter);
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
        // Include frame counter when running to force spinner redraws.
        if has_running {
            h = h.wrapping_mul(31).wrapping_add(self.frame_counter);
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

    fn compute_transcript_hash(&self, state: &TaskLayoutState) -> u64 {
        let mut h: u64 = state.output_rows.len() as u64;
        h = h
            .wrapping_mul(31)
            .wrapping_add(simple_hash(&state.output_title));
        h = h
            .wrapping_mul(31)
            .wrapping_add(state.output_scroll_offset as u64);
        h = h
            .wrapping_mul(31)
            .wrapping_add(match state.output_scroll_anchor {
                OutputScrollAnchor::Top => 1,
                OutputScrollAnchor::Bottom => 2,
            });
        for row in &state.output_rows {
            h = h.wrapping_mul(31).wrapping_add(simple_hash(row));
        }
        h
    }

    fn compute_composer_hash(&self, state: &TaskLayoutState) -> u64 {
        let mut h = simple_hash(&state.input_hint);
        h = h
            .wrapping_mul(31)
            .wrapping_add(simple_hash(&state.composer_text));
        h = h
            .wrapping_mul(31)
            .wrapping_add(state.composer_cursor as u64);
        if let Some(ref approval) = state.pending_approval {
            h = h.wrapping_mul(31).wrapping_add(simple_hash(approval));
        }
        h
    }
}

impl Default for TaskDraw {
    fn default() -> Self {
        Self::new()
    }
}

// ── Status line parsing ─────────────────────────────────────────────

struct StatusParts {
    mode: String,
    repo: String,
    inst: String,
    /// Cumulative session token count (0 when none have been recorded yet).
    tokens: u64,
    /// Pre-converted token count in thousands (computed once during parsing).
    tokens_k: f64,
}

fn parse_status_parts(status: &str) -> StatusParts {
    let mut mode = String::from("ready");
    let mut repo = String::from("vexcoder");
    let mut inst = String::from("none");
    let mut tokens: u64 = 0;

    for part in status.split_whitespace() {
        if let Some(val) = part.strip_prefix("mode:") {
            mode = val.to_string();
        } else if let Some(val) = part.strip_prefix("repo:") {
            repo = val.to_string();
        } else if let Some(val) = part.strip_prefix("inst:") {
            inst = val.to_string();
        } else if let Some(val) = part.strip_prefix("tokens:") {
            // `tokens:N` is written by status_line() as a plain decimal integer.
            // Any parse failure is treated as 0 (hides the indicator silently),
            // which is safe because the token count is optional display-only data.
            tokens = val.parse().unwrap_or(0);
        }
    }

    let tokens_k = tokens as f64 / 1000.0;
    StatusParts {
        mode,
        repo,
        inst,
        tokens,
        tokens_k,
    }
}

// ── Utilities ───────────────────────────────────────────────────────

fn transcript_window(state: &TaskLayoutState, viewport_height: usize) -> (usize, usize) {
    let total = state.output_rows.len();
    if viewport_height == 0 || total == 0 {
        return (0, 0);
    }

    match state.output_scroll_anchor {
        OutputScrollAnchor::Bottom => {
            let max_offset = total.saturating_sub(viewport_height);
            let offset = state.output_scroll_offset.min(max_offset);
            let start = total.saturating_sub(viewport_height.saturating_add(offset));
            let end = (start + viewport_height).min(total);
            (start, end)
        }
        OutputScrollAnchor::Top => {
            let start = state.output_scroll_offset.min(total.saturating_sub(1));
            let end = (start + viewport_height).min(total);
            (start, end)
        }
    }
}

/// Draw a labeled separator line at the given row.
fn draw_labeled_separator(w: &mut dyn Write, row: u16, cols: u16, label: &str) {
    move_to(w, row, 0);
    clear_line(w);
    set_dim(w);
    set_fg(w, DIM_GRAY);
    let safe_label = truncate_to_width(label, cols.saturating_sub(10) as usize);
    let left_rule = 3.min(cols as usize);
    for _ in 0..left_rule {
        let _ = write!(w, "\u{2500}");
    }
    if !safe_label.is_empty() {
        set_fg(w, BLUE);
        let _ = write!(w, " {safe_label} ");
        set_fg(w, DIM_GRAY);
    } else {
        let _ = write!(w, "\u{2500}");
    }

    let used = left_rule
        + if safe_label.is_empty() {
            1
        } else {
            display_width(&safe_label) + 2
        };
    for _ in 0..(cols as usize).saturating_sub(used).min(120) {
        let _ = write!(w, "\u{2500}");
    }
    reset_style(w);
}

/// Draw a thin scroll indicator on the right edge of the transcript area.
///
/// Uses Unicode block characters to show a thumb position proportional to
/// the visible window within the total content. The indicator occupies the
/// last column so it does not interfere with content rendering.
fn draw_scroll_indicator(
    w: &mut dyn Write,
    start_row: u16,
    viewport_height: usize,
    visible_start: usize,
    total_lines: usize,
    cols: u16,
) {
    if viewport_height == 0 || total_lines == 0 || cols < 2 {
        return;
    }
    let col = cols.saturating_sub(1);
    let thumb_height = ((viewport_height as f64 / total_lines as f64) * viewport_height as f64)
        .ceil()
        .max(1.0) as usize;
    let thumb_start =
        ((visible_start as f64 / total_lines as f64) * viewport_height as f64).round() as usize;

    for vp_offset in 0..viewport_height {
        let row = start_row + vp_offset as u16;
        move_to(w, row, col);
        if vp_offset >= thumb_start && vp_offset < thumb_start + thumb_height {
            set_fg(w, GRAY);
            let _ = write!(w, "\u{2588}"); // █ (thumb)
        } else {
            set_fg(w, DIM_GRAY);
            let _ = write!(w, "\u{2591}"); // ░ (track)
        }
        reset_style(w);
    }
}

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
        StepLifecycle::Queued => 0,
        StepLifecycle::Completed => 1,
        StepLifecycle::Failed => 2,
        StepLifecycle::Running => 3,
        StepLifecycle::AwaitingApproval => 4,
        StepLifecycle::Approved => 5,
        StepLifecycle::UserInput => 6,
        StepLifecycle::CommandSession => 7,
    }
}

fn composer_cursor_position(state: &TaskLayoutState, regions: &Regions) -> (u16, u16) {
    if state.pending_approval.is_some() {
        return (regions.composer_start, 0);
    }

    let input_width = regions.cols.saturating_sub(2).max(1) as usize;
    let (cursor_row, cursor_col) =
        cursor_row_col(&state.composer_text, state.composer_cursor, input_width);
    let body_rows = regions.composer_rows.saturating_sub(1).max(1) as usize;
    let window_start = visual_window_start(cursor_row, body_rows);
    let visible_row = cursor_row.saturating_sub(window_start) as u16;
    let row = regions
        .composer_start
        .saturating_add(1)
        .saturating_add(visible_row)
        .min(regions.rows.saturating_sub(1));
    let col = (2 + cursor_col as u16).min(regions.cols.saturating_sub(1));
    (row, col)
}
