//! Adaptive ANSI draw engine for the operator CLI surface.
//!
//! This module writes ANSI escape sequences directly to a `Write` sink,
//! owning the full screen for the entire session. The design goals are:
//!
//! 1. **Persistent full-screen ownership** — the draw engine owns the
//!    display at all times; the prompt is never yielded between tool
//!    calls or after a turn completes.
//! 2. **Flowing transcript** — tool calls, results, and model responses
//!    stream vertically in a continuous log, not a fixed-height window.
//!    Paragraphs scroll upward indefinitely consistent with ADR-031.
//! 3. **Adaptive layout** — the transcript body, composer, and bottom
//!    panes scale with the display dimensions.
//! 4. **Human-readable status** — the header shows plain-language state
//!    instead of machine-debug flags.
//! 5. **Minimal redraw** — only dirty regions are rewritten each frame.
//! 6. **Inline telemetry** — tool call activity and enriched responses
//!    are drawn with inline timing counters (ADR-040).
//!
//! The public entry point is [`TaskDraw`] which persists across the
//! session and is called from the `FrontendAdapter::render` path.

mod ansi;
mod regions;
pub(crate) mod transcript;

#[cfg(test)]
mod tests;

use crate::app::{OutputScrollAnchor, StepLifecycle, TaskLayoutState};
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
    /// Number of transcript lines already flushed to the display.
    output_lines_flushed: usize,
    /// Last rendered transcript content.
    last_transcript_hash: u64,
    /// Last rendered composer (for dirty detection).
    last_composer_hash: u64,
    /// Last rendered bottom pane content hash.
    last_bottom_pane_hash: u64,
    /// Display dimensions at last draw.
    last_cols: u16,
    last_rows: u16,
    /// Whether the very first frame has been drawn.
    first_frame_done: bool,
    /// Monotonic frame counter for spinner animation.
    frame_counter: u64,
    /// Whether the transcript is currently inside a code block.
    in_code_block: bool,
    /// Number of overlay rows drawn in the previous frame.
    last_overlay_rows: u16,
}

impl TaskDraw {
    pub fn new() -> Self {
        Self {
            output_lines_flushed: 0,
            last_transcript_hash: 0,
            last_composer_hash: 0,
            last_bottom_pane_hash: 0,
            last_cols: 0,
            last_rows: 0,
            first_frame_done: false,
            frame_counter: 0,
            in_code_block: false,
            last_overlay_rows: 0,
        }
    }

    /// Reset for a new turn (keeps display state but resets line counters).
    pub fn reset(&mut self) {
        self.output_lines_flushed = 0;
        self.last_transcript_hash = 0;
        self.last_composer_hash = 0;
        self.last_bottom_pane_hash = 0;
        self.first_frame_done = false;
        self.in_code_block = false;
        self.last_overlay_rows = 0;
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
        self.last_cols = term_cols;
        self.last_rows = term_rows;

        let has_files = !state.changed_files.is_empty();
        let regions = Regions::compute_with_composer(
            term_cols,
            term_rows,
            has_files,
            state.timeline_entries.len(),
            &state.composer_text,
        );

        // On first frame or display resize: full repaint.
        if !self.first_frame_done || size_changed {
            hide_cursor(w);
            self.draw_full(w, state, &regions);
            self.first_frame_done = true;
            let _ = w.flush();
            return;
        }

        // Incremental update: only redraw dirty regions.
        hide_cursor(w);

        // Transcript (top, scrollable).
        let transcript_hash = self.compute_transcript_hash(state);
        if transcript_hash != self.last_transcript_hash {
            if self.transcript_is_append_only(state) {
                self.draw_transcript_incremental(w, state, &regions);
            } else {
                self.draw_transcript_full(w, state, &regions);
            }
            self.last_transcript_hash = transcript_hash;
        }

        // Bottom panes (telemetry + git, fixed height).
        let bottom_pane_hash = self.compute_bottom_pane_hash(state);
        if bottom_pane_hash != self.last_bottom_pane_hash {
            self.draw_bottom_panes(w, state, &regions);
            self.last_bottom_pane_hash = bottom_pane_hash;
        }

        // Composer (bottom, persistent prompt area).
        let composer_hash = self.compute_composer_hash(state);
        if composer_hash != self.last_composer_hash {
            self.draw_composer(w, state, &regions);
            self.last_composer_hash = composer_hash;
        }

        // Picker overlay (overlays above the composer).
        self.draw_picker_overlay(w, state, &regions);

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

        self.draw_transcript_full(w, state, regions);
        self.last_transcript_hash = self.compute_transcript_hash(state);

        self.draw_bottom_panes(w, state, regions);
        self.last_bottom_pane_hash = self.compute_bottom_pane_hash(state);

        self.draw_composer(w, state, regions);
        self.last_composer_hash = self.compute_composer_hash(state);

        self.draw_picker_overlay(w, state, regions);

        self.draw_status_bar(w, state, regions);

        let (cursor_row, cursor_col) = composer_cursor_position(state, regions);
        move_to(w, cursor_row, cursor_col);
        show_cursor(w);
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
        let render_start_row =
            transcript_render_start_row(state, regions, visible_start, visible_end);

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
            let row = render_start_row + vp_offset as u16;
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
        let render_start_row =
            transcript_render_start_row(state, regions, visible_start, visible_end);

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
            let row = render_start_row + vp_offset as u16;
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
            let composer_char_count = state.composer_text.chars().count();

            if state.composer_focused {
                set_bold(w);
                set_fg(w, WHITE);
            } else {
                set_dim(w);
                set_fg(w, DIM_GRAY);
            }
            let _ = write!(w, "Prompt");
            reset_style(w);

            let status = format!(
                "{} · {} chars",
                if state.composer_focused {
                    "focused"
                } else {
                    "unfocused"
                },
                composer_char_count
            );
            let status_width = display_width(&status);
            let prompt_width = display_width("Prompt");
            if status_width + prompt_width + 1 < regions.cols as usize {
                let gap = (regions.cols as usize).saturating_sub(prompt_width + status_width);
                for _ in 0..gap {
                    let _ = write!(w, " ");
                }
                set_dim(w);
                set_fg(
                    w,
                    if state.composer_focused {
                        CYAN
                    } else {
                        DIM_GRAY
                    },
                );
                let _ = write!(w, "{status}");
                reset_style(w);
            }

            for offset in 0..body_rows {
                let row = regions.composer_start + 1 + offset as u16;
                if row >= regions.rows {
                    break;
                }
                move_to(w, row, 0);

                set_fg(
                    w,
                    if state.composer_focused {
                        CYAN
                    } else {
                        DIM_GRAY
                    },
                );
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

    // ── Bottom panes (telemetry + git) ────────────────────────────

    fn draw_bottom_panes<W: Write>(&self, w: &mut W, state: &TaskLayoutState, regions: &Regions) {
        if regions.bottom_pane_rows == 0 {
            return;
        }

        let split = regions.bottom_pane_split_col as usize;
        let cols = regions.cols as usize;

        // Clear the bottom pane area.
        for i in 0..regions.bottom_pane_rows {
            let row = regions.bottom_pane_start + i;
            if row >= regions.rows {
                break;
            }
            move_to(w, row, 0);
            clear_line(w);
        }

        // ── Left half: telemetry ──
        // Show a header and any available timing data from status_line.
        move_to(w, regions.bottom_pane_start, 0);
        set_dim(w);
        set_fg(w, CYAN);
        let _ = write!(w, " Telemetry");
        reset_style(w);

        // Parse timing data from status_line if present.
        let status = &state.status_line;
        let mut telem_lines: Vec<String> = Vec::new();
        for part in status.split_whitespace() {
            if let Some(val) = part.strip_prefix("tokens:") {
                telem_lines.push(format!("tokens: {val}"));
            } else if let Some(val) = part.strip_prefix("mode:") {
                telem_lines.push(format!("mode: {val}"));
            }
        }
        if telem_lines.is_empty() {
            telem_lines.push("waiting…".to_string());
        }
        let left_width = split.saturating_sub(2);
        for (i, line) in telem_lines
            .iter()
            .take(regions.bottom_pane_rows.saturating_sub(1) as usize)
            .enumerate()
        {
            let row = regions.bottom_pane_start + 1 + i as u16;
            if row >= regions.bottom_pane_start + regions.bottom_pane_rows {
                break;
            }
            move_to(w, row, 1);
            set_fg(w, GRAY);
            let truncated = truncate_to_width(line, left_width);
            let _ = write!(w, "{truncated}");
            reset_style(w);
        }

        // ── Separator ──
        for i in 0..regions.bottom_pane_rows {
            let row = regions.bottom_pane_start + i;
            if row >= regions.rows {
                break;
            }
            move_to(w, row, regions.bottom_pane_split_col);
            set_fg(w, DIM_GRAY);
            let _ = write!(w, "\u{2502}"); // │
            reset_style(w);
        }

        // ── Right half: git / changed files ──
        move_to(
            w,
            regions.bottom_pane_start,
            regions.bottom_pane_split_col + 1,
        );
        set_dim(w);
        set_fg(w, CYAN);
        let _ = write!(w, " Files");
        reset_style(w);

        let right_start = (regions.bottom_pane_split_col + 2) as usize;
        let right_width = cols.saturating_sub(right_start + 1);
        if state.changed_files.is_empty() {
            let row = regions.bottom_pane_start + 1;
            if row < regions.bottom_pane_start + regions.bottom_pane_rows {
                move_to(w, row, right_start as u16);
                set_fg(w, DIM_GRAY);
                let _ = write!(w, "no changes");
                reset_style(w);
            }
        } else {
            for (i, file) in state
                .changed_files
                .iter()
                .take(regions.bottom_pane_rows.saturating_sub(1) as usize)
                .enumerate()
            {
                let row = regions.bottom_pane_start + 1 + i as u16;
                if row >= regions.bottom_pane_start + regions.bottom_pane_rows {
                    break;
                }
                move_to(w, row, right_start as u16);
                set_fg(w, GRAY);
                let truncated = truncate_to_width(file, right_width);
                let _ = write!(w, "{truncated}");
                reset_style(w);
            }
        }
    }

    // ── Picker overlay ────────────────────────────────────────────

    fn draw_picker_overlay<W: Write>(
        &mut self,
        w: &mut W,
        state: &TaskLayoutState,
        regions: &Regions,
    ) {
        let max_rows = (regions.transcript_rows as usize).min(14);
        let visible = state.picker_overlay.len().min(max_rows) as u16;

        // Clear stale overlay rows from the previous frame.
        // Picker renders upward from just above the composer (bottom), overlaying
        // the bottom panes / transcript area.
        let overlay_start = regions.composer_start.saturating_sub(visible);
        if self.last_overlay_rows > visible {
            let old_start = regions
                .composer_start
                .saturating_sub(self.last_overlay_rows);
            for row in old_start..overlay_start {
                if row < regions.transcript_start {
                    continue;
                }
                move_to(w, row, 0);
                clear_line(w);
            }
        }
        self.last_overlay_rows = visible;

        if visible == 0 {
            return;
        }

        for (i, line) in state
            .picker_overlay
            .iter()
            .take(visible as usize)
            .enumerate()
        {
            let row = overlay_start + i as u16;
            if row >= regions.composer_start {
                break;
            }
            move_to(w, row, 0);
            clear_line(w);
            if line.selected {
                set_bold(w);
                set_fg(w, CYAN);
            } else {
                set_dim(w);
                set_fg(w, DIM_GRAY);
            }
            let truncated = truncate_to_width(&line.text, regions.cols as usize);
            let _ = write!(w, "{truncated}");
            reset_style(w);
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
            " PgUp/PgDn transcript  Shift+Enter newline  Enter submit"
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

    fn compute_bottom_pane_hash(&self, state: &TaskLayoutState) -> u64 {
        let mut h: u64 = simple_hash(&state.status_line);
        h = h
            .wrapping_mul(31)
            .wrapping_add(state.changed_files.len() as u64);
        for f in &state.changed_files {
            h = h.wrapping_mul(31).wrapping_add(simple_hash(f));
        }
        // Include frame counter so spinner animates.
        h = h.wrapping_mul(31).wrapping_add(self.frame_counter);
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
        h = h
            .wrapping_mul(31)
            .wrapping_add(state.composer_focused as u64);
        if let Some(ref approval) = state.pending_approval {
            h = h.wrapping_mul(31).wrapping_add(simple_hash(approval));
        }
        // Include picker overlay in hash so overlay redraws on picker changes.
        h = h
            .wrapping_mul(31)
            .wrapping_add(state.picker_overlay.len() as u64);
        for line in &state.picker_overlay {
            h = h.wrapping_mul(31).wrapping_add(simple_hash(&line.text));
            h = h.wrapping_mul(31).wrapping_add(line.selected as u64);
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

#[cfg(test)]
struct StatusParts {
    mode: String,
    repo: String,
    inst: String,
    /// Cumulative session token count (0 when none have been recorded yet).
    tokens: u64,
}

#[cfg(test)]
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

    StatusParts {
        mode,
        repo,
        inst,
        tokens,
    }
}

// ── Utilities ───────────────────────────────────────────────────────

fn transcript_window(state: &TaskLayoutState, viewport_height: usize) -> (usize, usize) {
    const INSPECTOR_VIEWPORT_ROWS: usize = 6;

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
            let inspector_height = viewport_height.clamp(1, INSPECTOR_VIEWPORT_ROWS);
            let start = state.output_scroll_offset.min(total.saturating_sub(1));
            let end = (start + inspector_height).min(total);
            (start, end)
        }
    }
}

fn transcript_render_start_row(
    state: &TaskLayoutState,
    regions: &Regions,
    visible_start: usize,
    visible_end: usize,
) -> u16 {
    let visible_len = visible_end.saturating_sub(visible_start);
    if state.output_scroll_anchor == OutputScrollAnchor::Bottom
        && visible_len < regions.transcript_rows as usize
    {
        return regions
            .transcript_start
            .saturating_add(regions.transcript_rows.saturating_sub(visible_len as u16));
    }

    regions.transcript_start
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
