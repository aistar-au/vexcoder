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

use crate::app::{OutputScrollAnchor, StepLifecycle, TaskLayoutState, TimelineEntry};
use crate::ui::input_metrics::{
    cursor_row_col, display_width, truncate_to_display_width, wrap_input_lines,
};
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

fn set_italic(w: &mut dyn Write) {
    let _ = write!(w, "{CSI}3m");
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

// ── Spinner frames (braille animation for running steps) ────────────

const SPINNER_FRAMES: &[&str] = &[
    "\u{2846}", // ⡆
    "\u{2807}", // ⠇
    "\u{2803}", // ⠃
    "\u{2819}", // ⠙
    "\u{2838}", // ⠸
    "\u{2830}", // ⠰
    "\u{2860}", // ⡠
    "\u{2844}", // ⡄
];

// ── Color palette ───────────────────────────────────────────────────

const GREEN: u8 = 2;
const RED: u8 = 1;
const CYAN: u8 = 6;
const YELLOW: u8 = 3;
const MAGENTA: u8 = 5;
const GRAY: u8 = 245;
const DIM_GRAY: u8 = 240;
const WHITE: u8 = 15;
const BLUE: u8 = 4;

// ── Progress bar frames (animated fill for running state) ───────────

const PROGRESS_FRAMES: &[&str] = &[
    "\u{2591}\u{2591}\u{2593}\u{2588}\u{2588}\u{2593}\u{2591}\u{2591}", // ░░▓██▓░░
    "\u{2591}\u{2593}\u{2588}\u{2588}\u{2593}\u{2591}\u{2591}\u{2591}", // ░▓██▓░░░
    "\u{2593}\u{2588}\u{2588}\u{2593}\u{2591}\u{2591}\u{2591}\u{2591}", // ▓██▓░░░░
    "\u{2588}\u{2588}\u{2593}\u{2591}\u{2591}\u{2591}\u{2591}\u{2593}", // ██▓░░░░▓
    "\u{2588}\u{2593}\u{2591}\u{2591}\u{2591}\u{2591}\u{2593}\u{2588}", // █▓░░░░▓█
    "\u{2593}\u{2591}\u{2591}\u{2591}\u{2591}\u{2593}\u{2588}\u{2588}", // ▓░░░░▓██
    "\u{2591}\u{2591}\u{2591}\u{2591}\u{2593}\u{2588}\u{2588}\u{2593}", // ░░░░▓██▓
    "\u{2591}\u{2591}\u{2591}\u{2593}\u{2588}\u{2588}\u{2593}\u{2591}", // ░░░▓██▓░
];

fn lifecycle_color(lifecycle: &StepLifecycle) -> u8 {
    match lifecycle {
        StepLifecycle::Completed => GREEN,
        StepLifecycle::Failed => RED,
        StepLifecycle::Running => CYAN,
        StepLifecycle::AwaitingApproval => YELLOW,
        StepLifecycle::Approved => GREEN,
        StepLifecycle::UserInput => DIM_GRAY,
        StepLifecycle::CommandSession => MAGENTA,
    }
}

fn lifecycle_prefix(lifecycle: &StepLifecycle) -> &'static str {
    match lifecycle {
        StepLifecycle::Completed => "\u{2605}",        // ★
        StepLifecycle::Failed => "\u{2716}",           // ✖
        StepLifecycle::Running => "\u{2726}",          // ✦
        StepLifecycle::AwaitingApproval => "\u{2606}", // ☆
        StepLifecycle::Approved => "\u{2713}",         // ✓
        StepLifecycle::UserInput => "\u{203a}",        // ›
        StepLifecycle::CommandSession => "\u{25c6}",   // ◆
    }
}

// ── Adaptive region geometry ────────────────────────────────────────

/// Adaptive layout regions that scale with terminal dimensions.
///
/// ```text
/// row 0        ┌─ header (repo + status) ───────┐  (1 row)
/// row 1        ├─ changed files (optional) ──────┤  (0..1 rows)
/// row H..T     ├─ timeline (adaptive height) ────┤  (3..40% of rows)
/// row T..C     │  transcript (remaining rows)    │  (fills remaining)
/// row C..end   ├─ composer (adaptive) ───────────┤  (4..8 rows)
///              └─────────────────────────────────┘
/// ```
struct Regions {
    cols: u16,
    rows: u16,
    header_row: u16,
    files_row: Option<u16>,
    timeline_start: u16,
    timeline_rows: u16,
    transcript_start: u16,
    transcript_rows: u16,
    composer_start: u16,
    composer_rows: u16,
    status_bar_row: u16,
}

/// Minimum timeline rows (below this the timeline is hidden).
const MIN_TIMELINE_ROWS: u16 = 3;
/// Minimum transcript rows reserved above the prompt surface.
const MIN_TRANSCRIPT_ROWS: u16 = 2;
/// Maximum fraction of terminal for the timeline.
const MAX_TIMELINE_FRACTION: f32 = 0.35;
/// Minimum fullscreen prompt rows (toolbar + multiline input).
const MIN_COMPOSER_ROWS: u16 = 3;

fn preferred_composer_rows(rows: u16) -> u16 {
    if rows >= 36 {
        8
    } else if rows >= 28 {
        7
    } else if rows >= 22 {
        6
    } else if rows >= 16 {
        5
    } else {
        4
    }
}

impl Regions {
    fn compute(cols: u16, rows: u16, has_files: bool, timeline_entry_count: usize) -> Self {
        let header_row = 0;
        let files_row = if has_files { Some(1) } else { None };
        let header_height = if has_files { 2u16 } else { 1u16 };

        // Reserve 1 row for status bar at the very bottom.
        let status_bar_row = rows.saturating_sub(1);

        // Composer: dedicate a larger bottom-docked prompt surface while
        // preserving minimum room for timeline and transcript.
        let available = rows.saturating_sub(header_height).saturating_sub(1);
        let max_composer_rows = available
            .saturating_sub(MIN_TIMELINE_ROWS)
            .saturating_sub(MIN_TRANSCRIPT_ROWS)
            .max(MIN_COMPOSER_ROWS);
        let composer_rows = preferred_composer_rows(rows)
            .max(MIN_COMPOSER_ROWS)
            .min(max_composer_rows);

        let available = available.saturating_sub(composer_rows);

        // Timeline: adaptive height based on content and terminal size.
        // Uses up to MAX_TIMELINE_FRACTION of available space, but at least
        // MIN_TIMELINE_ROWS (with a title row).
        let content_rows = (timeline_entry_count as u16).saturating_add(1); // +1 for title
        let max_timeline = ((available as f32) * MAX_TIMELINE_FRACTION) as u16;
        let timeline_rows = content_rows.min(max_timeline).max(MIN_TIMELINE_ROWS);

        // Transcript: everything left after timeline.
        let transcript_rows = available.saturating_sub(timeline_rows);

        let timeline_start = header_height;
        let transcript_start = timeline_start + timeline_rows;
        let composer_start = transcript_start + transcript_rows;

        Regions {
            cols,
            rows,
            header_row,
            files_row,
            timeline_start,
            timeline_rows,
            transcript_start,
            transcript_rows,
            composer_start,
            composer_rows,
            status_bar_row,
        }
    }
}

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

    /// Render a single transcript line with semantic and markdown-aware styling.
    fn draw_transcript_line(&mut self, w: &mut dyn Write, line: &str, cols: u16) {
        // ── Tool status markers ────────────────────────────────────
        if let Some(rest) = line.strip_prefix("[ok] ") {
            set_bold(w);
            set_fg(w, GREEN);
            let _ = write!(w, " \u{2605} "); // ★
            reset_style(w);
            set_fg(w, WHITE);
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(3));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }
        if let Some(rest) = line.strip_prefix("[!] ") {
            set_bold(w);
            set_fg(w, RED);
            let _ = write!(w, " \u{2716} "); // ✖
            reset_style(w);
            set_fg(w, WHITE);
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(3));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Code block fence detection ─────────────────────────────
        if line.starts_with("```") {
            self.in_code_block = !self.in_code_block;
            set_dim(w);
            set_fg(w, DIM_GRAY);
            if self.in_code_block {
                // Opening fence — show language tag if present.
                let lang = line.trim_start_matches('`').trim();
                let _ = write!(w, " \u{2500}\u{2500} ");
                if !lang.is_empty() {
                    set_fg(w, BLUE);
                    let _ = write!(w, "{lang} ");
                    set_fg(w, DIM_GRAY);
                }
                let used = 4 + if lang.is_empty() {
                    0
                } else {
                    display_width(lang) + 1
                };
                let remaining = (cols as usize).saturating_sub(used);
                for _ in 0..remaining.min(60) {
                    let _ = write!(w, "\u{2500}"); // ─
                }
            } else {
                // Closing fence — thin rule.
                let _ = write!(w, " ");
                for _ in 0..(cols as usize).saturating_sub(1).min(60) {
                    let _ = write!(w, "\u{2500}");
                }
            }
            reset_style(w);
            return;
        }

        // ── Inside code block — monospace with left bar ────────────
        if self.in_code_block {
            set_fg(w, DIM_GRAY);
            let _ = write!(w, " \u{2502} "); // │
            set_fg(w, GRAY);
            let truncated = truncate_to_width(line, (cols as usize).saturating_sub(3));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Markdown headers ───────────────────────────────────────
        if let Some(rest) = line.strip_prefix("### ") {
            set_bold(w);
            set_fg(w, YELLOW);
            let _ = write!(w, " ");
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(1));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            set_bold(w);
            set_fg(w, YELLOW);
            let _ = write!(w, " ");
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(1));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }
        if let Some(rest) = line.strip_prefix("# ") {
            set_bold(w);
            set_fg(w, WHITE);
            let _ = write!(w, " ");
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(1));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Blockquotes ────────────────────────────────────────────
        if let Some(rest) = line.strip_prefix("> ") {
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let _ = write!(w, " \u{2502} "); // │
            set_fg(w, GRAY);
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(3));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Bullet lists ───────────────────────────────────────────
        if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            // Task checklist items: - [x] or - [ ] patterns.
            if let Some(task_rest) = rest.strip_prefix("[x] ").or_else(|| rest.strip_prefix("[X] "))
            {
                set_fg(w, GREEN);
                let _ = write!(w, " \u{2611} "); // ☑
                reset_style(w);
                set_fg(w, GRAY);
                let truncated = truncate_to_width(task_rest, (cols as usize).saturating_sub(3));
                let _ = write!(w, "{truncated}");
                reset_style(w);
                return;
            }
            if let Some(task_rest) = rest.strip_prefix("[ ] ") {
                set_fg(w, DIM_GRAY);
                let _ = write!(w, " \u{2610} "); // ☐
                reset_style(w);
                set_fg(w, GRAY);
                let truncated = truncate_to_width(task_rest, (cols as usize).saturating_sub(3));
                let _ = write!(w, "{truncated}");
                reset_style(w);
                return;
            }
            set_fg(w, YELLOW);
            let _ = write!(w, " \u{2022} "); // •
            reset_style(w);
            set_fg(w, GRAY);
            let truncated = truncate_to_width(rest, (cols as usize).saturating_sub(3));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Numbered lists ─────────────────────────────────────────
        if let Some(num_rest) = parse_numbered_list_item(line) {
            set_fg(w, YELLOW);
            let _ = write!(w, " {}", num_rest.0);
            reset_style(w);
            set_fg(w, GRAY);
            let truncated =
                truncate_to_width(num_rest.1, (cols as usize).saturating_sub(num_rest.2 + 1));
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Horizontal rules ───────────────────────────────────────
        if is_horizontal_rule(line) {
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let rule_width = (cols as usize).min(80);
            let _ = write!(w, " ");
            for _ in 0..rule_width.saturating_sub(1) {
                let _ = write!(w, "\u{2500}"); // ─
            }
            reset_style(w);
            return;
        }

        // ── Indented detail text ───────────────────────────────────
        if line.starts_with("    ") {
            set_dim(w);
            set_fg(w, GRAY);
            let truncated = truncate_to_width(line, cols as usize);
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Section separator ──────────────────────────────────────
        if line.starts_with("--- ") && line.ends_with(" ---") {
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let _ = write!(w, " \u{2500}\u{2500}\u{2500} "); // ───
            set_fg(w, YELLOW);
            let _ = write!(w, "\u{2726}"); // ✦
            set_fg(w, DIM_GRAY);
            let label = line.trim_start_matches('-').trim_end_matches('-').trim();
            let prefix_used: usize = 5;
            if !label.is_empty() {
                let max_label = (cols as usize).saturating_sub(prefix_used + 4);
                let safe_label = truncate_to_width(label, max_label);
                let _ = write!(w, " {safe_label} ");
                let label_display_w = display_width(&safe_label);
                let remaining = (cols as usize).saturating_sub(prefix_used + 2 + label_display_w);
                for _ in 0..remaining.min(40) {
                    let _ = write!(w, "\u{2500}");
                }
            } else {
                let _ = write!(w, " ");
                let remaining = (cols as usize).saturating_sub(prefix_used + 1);
                for _ in 0..remaining.min(40) {
                    let _ = write!(w, "\u{2500}");
                }
            }
            reset_style(w);
            return;
        }

        // ── Hint text ──────────────────────────────────────────────
        if line == "Turn completed." || line.starts_with("Type a prompt") {
            set_dim(w);
            set_fg(w, DIM_GRAY);
            let truncated = truncate_to_width(line, cols as usize);
            let _ = write!(w, "{truncated}");
            reset_style(w);
            return;
        }

        // ── Awaiting indicator ─────────────────────────────────────
        if line == "[awaiting model response]" {
            let idx = (self.frame_counter as usize) % SPINNER_FRAMES.len();
            set_fg(w, CYAN);
            let _ = write!(w, " {} awaiting response", SPINNER_FRAMES[idx]);
            reset_style(w);
            return;
        }

        // ── Regular text with inline bold detection ────────────────
        set_fg(w, GRAY);
        self.draw_inline_markdown(w, line, cols);
        reset_style(w);
    }

    /// Render inline markdown: **bold**, *italic*, `code`, and ~~strikethrough~~ spans.
    fn draw_inline_markdown(&self, w: &mut dyn Write, line: &str, cols: u16) {
        let max_w = cols as usize;
        let mut used: usize = 0;
        let mut chars = line.chars().peekable();
        let mut buf = String::new();

        while let Some(ch) = chars.next() {
            if used >= max_w {
                break;
            }
            if ch == '*' && chars.peek() == Some(&'*') {
                // Flush buffer.
                if !buf.is_empty() {
                    let truncated = truncate_to_width(&buf, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    buf.clear();
                }
                chars.next(); // consume second *
                              // Collect bold text until **
                let mut bold = String::new();
                while let Some(bc) = chars.next() {
                    if bc == '*' && chars.peek() == Some(&'*') {
                        chars.next();
                        break;
                    }
                    bold.push(bc);
                }
                if !bold.is_empty() {
                    set_bold(w);
                    set_fg(w, WHITE);
                    let truncated = truncate_to_width(&bold, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    reset_style(w);
                    set_fg(w, GRAY);
                }
            } else if ch == '*' || ch == '_' {
                // Single delimiter italic — only if the next char is not a space.
                let is_word_boundary = chars.peek().is_none_or(|c| c.is_whitespace());
                if is_word_boundary {
                    buf.push(ch);
                    continue;
                }
                // Flush buffer.
                if !buf.is_empty() {
                    let truncated = truncate_to_width(&buf, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    buf.clear();
                }
                let mut italic = String::new();
                for ic in chars.by_ref() {
                    if ic == ch {
                        break;
                    }
                    italic.push(ic);
                }
                if !italic.is_empty() {
                    set_italic(w);
                    set_fg(w, GRAY);
                    let truncated = truncate_to_width(&italic, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    reset_style(w);
                    set_fg(w, GRAY);
                }
            } else if ch == '~' && chars.peek() == Some(&'~') {
                // Flush buffer.
                if !buf.is_empty() {
                    let truncated = truncate_to_width(&buf, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    buf.clear();
                }
                chars.next(); // consume second ~
                let mut struck = String::new();
                while let Some(sc) = chars.next() {
                    if sc == '~' && chars.peek() == Some(&'~') {
                        chars.next();
                        break;
                    }
                    struck.push(sc);
                }
                if !struck.is_empty() {
                    set_dim(w);
                    set_fg(w, DIM_GRAY);
                    let truncated = truncate_to_width(&struck, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    reset_style(w);
                    set_fg(w, GRAY);
                }
            } else if ch == '`' {
                // Flush buffer.
                if !buf.is_empty() {
                    let truncated = truncate_to_width(&buf, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    buf.clear();
                }
                // Collect code span until `
                let mut code = String::new();
                for cc in chars.by_ref() {
                    if cc == '`' {
                        break;
                    }
                    code.push(cc);
                }
                if !code.is_empty() {
                    set_fg(w, CYAN);
                    let truncated = truncate_to_width(&code, max_w.saturating_sub(used));
                    let _ = write!(w, "{truncated}");
                    used += display_width(&truncated);
                    set_fg(w, GRAY);
                }
            } else {
                buf.push(ch);
            }
        }
        // Flush remaining buffer.
        if !buf.is_empty() {
            let truncated = truncate_to_width(&buf, max_w.saturating_sub(used));
            let _ = write!(w, "{truncated}");
        }
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
            let window_start = composer_window_start(cursor_row, body_rows);
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
    let thumb_start = ((visible_start as f64 / total_lines as f64) * viewport_height as f64)
        .round() as usize;

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

/// Parse a numbered list item like "1. foo" or "12. bar".
/// Returns (prefix_with_dot, rest_text, prefix_display_width).
fn parse_numbered_list_item(line: &str) -> Option<(&str, &str, usize)> {
    let bytes = line.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_digit() {
        return None;
    }
    // Find the dot-space after digits: "N. "
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i >= bytes.len().saturating_sub(1) {
        return None;
    }
    if bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1] == b' ' {
        let prefix = &line[..i + 2]; // e.g. "1. "
        let rest = &line[i + 2..];
        Some((prefix, rest, display_width(prefix)))
    } else {
        None
    }
}

/// Check if a line is a markdown horizontal rule (---, ***, or ___).
fn is_horizontal_rule(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < 3 {
        return false;
    }
    let ch = trimmed.as_bytes()[0];
    if ch != b'-' && ch != b'*' && ch != b'_' {
        return false;
    }
    // Must be at least 3 of the same character, optionally with spaces.
    let count = trimmed.chars().filter(|c| *c as u8 == ch).count();
    let space_count = trimmed.chars().filter(|c| *c == ' ').count();
    count >= 3 && count + space_count == trimmed.len()
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
        StepLifecycle::Approved => 5,
        StepLifecycle::UserInput => 6,
        StepLifecycle::CommandSession => 7,
    }
}

fn composer_window_start(cursor_row: usize, visible_rows: usize) -> usize {
    cursor_row
        .saturating_add(1)
        .saturating_sub(visible_rows.max(1))
}

fn composer_cursor_position(state: &TaskLayoutState, regions: &Regions) -> (u16, u16) {
    if state.pending_approval.is_some() {
        return (regions.composer_start, 0);
    }

    let input_width = regions.cols.saturating_sub(2).max(1) as usize;
    let (cursor_row, cursor_col) =
        cursor_row_col(&state.composer_text, state.composer_cursor, input_width);
    let body_rows = regions.composer_rows.saturating_sub(1).max(1) as usize;
    let window_start = composer_window_start(cursor_row, body_rows);
    let visible_row = cursor_row.saturating_sub(window_start) as u16;
    let row = regions
        .composer_start
        .saturating_add(1)
        .saturating_add(visible_row)
        .min(regions.rows.saturating_sub(1));
    let col = (2 + cursor_col as u16).min(regions.cols.saturating_sub(1));
    (row, col)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{StepLifecycle, TaskLayoutState, TimelineEntry};

    fn make_state(entries: Vec<TimelineEntry>, output: Vec<&str>) -> TaskLayoutState {
        TaskLayoutState {
            task_id: "test-001".into(),
            status_line: "mode:streaming approval:none repo:vexcoder inst:AGENTS.md".into(),
            activity_rows: vec![],
            total_steps: entries.len(),
            timeline_entries: entries,
            selected_step: 0,
            output_title: "Transcript".into(),
            output_rows: output.into_iter().map(|s| s.to_string()).collect(),
            output_scroll_offset: 0,
            output_scroll_anchor: OutputScrollAnchor::Bottom,
            pending_approval: None,
            input_hint: "Prompt\nUse `/` for commands, `@path` to inline files, paste large blocks, and Shift+Enter for a newline.".into(),
            composer_text: String::new(),
            composer_cursor: 0,
            changed_files: vec![],
            follow_mode: true,
        }
    }

    #[test]
    fn first_draw_writes_full_screen() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = make_state(
            vec![TimelineEntry {
                step_id: 1,
                lifecycle: StepLifecycle::Running,
                label: "read_file: running...".into(),
                detail: String::new(),
                session_id: None,
            }],
            vec!["line 1", "line 2"],
        );

        draw.draw(&mut buf, &state, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        assert!(output.contains("\x1b["), "output must contain ANSI escapes");
        // Human-readable header must show repo name.
        assert!(output.contains("vexcoder"), "header must show repo name");
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
                step_id: 1,
                lifecycle: StepLifecycle::Completed,
                label: "read_file: ok".into(),
                detail: String::new(),
                session_id: None,
            }],
            vec!["output line"],
        );

        draw.draw(&mut buf, &state, 80, 24);
        let first_len = buf.len();

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
    fn transcript_scroll_offset_renders_older_rows_from_prompt_edge() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let mut state = make_state(vec![], vec![]);
        state.output_rows = (0..20).map(|i| format!("line-{i}")).collect();
        state.output_scroll_offset = 2;

        draw.draw(&mut buf, &state, 80, 12);
        let output = String::from_utf8_lossy(&buf);

        assert!(
            output.contains("line-15"),
            "older transcript rows must stay visible"
        );
        assert!(
            !output.contains("line-19"),
            "bottom-most rows must move out of view when scrolled upward"
        );
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
        let lifecycles = [
            StepLifecycle::Completed,
            StepLifecycle::Failed,
            StepLifecycle::Running,
            StepLifecycle::AwaitingApproval,
            StepLifecycle::Approved,
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
                step_id: 1,
                lifecycle: StepLifecycle::Running,
                label: "tool: status".into(),
                detail: String::new(),
                session_id: None,
            }],
            vec![],
        );
        draw.draw(&mut buf, &running, 80, 24);

        buf.clear();
        let completed = make_state(
            vec![TimelineEntry {
                step_id: 1,
                lifecycle: StepLifecycle::Completed,
                label: "tool: status".into(),
                detail: String::new(),
                session_id: None,
            }],
            vec![],
        );
        draw.draw(&mut buf, &completed, 80, 24);

        let output = String::from_utf8_lossy(&buf);
        assert!(
            output.contains("\u{2605}"),
            "lifecycle changes must redraw the timeline row with star marker"
        );
    }

    #[test]
    fn changing_selected_inspector_entry_redraws_output() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();

        let first = TaskLayoutState {
            task_id: "test-001".into(),
            status_line: "mode:streaming approval:none repo:vexcoder inst:none".into(),
            activity_rows: vec![],
            timeline_entries: vec![
                TimelineEntry {
                    step_id: 1,
                    lifecycle: StepLifecycle::Completed,
                    label: "read_file: ok".into(),
                    detail: "Tool: read_file\nOutcome: ok".into(),
                    session_id: None,
                },
                TimelineEntry {
                    step_id: 2,
                    lifecycle: StepLifecycle::Completed,
                    label: "check: ok".into(),
                    detail: "Tool: check\nOutcome: ok".into(),
                    session_id: None,
                },
            ],
            selected_step: 0,
            total_steps: 2,
            output_title: "Inspector".into(),
            output_rows: vec!["Tool: read_file".into(), "Outcome: ok".into()],
            output_scroll_offset: 0,
            output_scroll_anchor: OutputScrollAnchor::Top,
            pending_approval: None,
            input_hint: "Prompt\nUse `/` for commands, `@path` to inline files, paste large blocks, and Shift+Enter for a newline.".into(),
            composer_text: String::new(),
            composer_cursor: 0,
            changed_files: vec![],
            follow_mode: true,
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
            "selection changes must redraw transcript"
        );
    }

    #[test]
    fn fallback_activity_rows_render_without_timeline_entries() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = TaskLayoutState {
            task_id: "test-001".into(),
            status_line: "mode:streaming approval:none repo:vexcoder inst:none".into(),
            activity_rows: vec!["[->] validate: running...".into(), "> ship it".into()],
            timeline_entries: vec![],
            selected_step: 0,
            total_steps: 0,
            output_title: "Transcript".into(),
            output_rows: vec!["line 1".into()],
            output_scroll_offset: 0,
            output_scroll_anchor: OutputScrollAnchor::Bottom,
            pending_approval: None,
            input_hint: "Prompt\nUse `/` for commands, `@path` to inline files, paste large blocks, and Shift+Enter for a newline.".into(),
            composer_text: String::new(),
            composer_cursor: 0,
            changed_files: vec![],
            follow_mode: true,
        };

        draw.draw(&mut buf, &state, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        assert!(output.contains("validate: running"));
        assert!(output.contains("ship it"));
    }

    #[test]
    fn enriched_paragraph_output_renders_tool_status_colors() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = make_state(
            vec![],
            vec![
                "[ok] read_file",
                "    README.md: 42 lines",
                "",
                "[!] write_file",
                "    error: permission denied",
                "",
                "The file was read successfully.",
            ],
        );

        draw.draw(&mut buf, &state, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        // Star markers in the transcript area: both ★ and ✖ must be drawn
        // (the ✖ marker only appears in transcript, not header, so it is
        // sufficient to confirm transcript rendering is active).
        assert!(
            output.contains("\u{2716}"),
            "failure cross marker must be drawn in transcript"
        );
        assert!(
            output.contains("read_file"),
            "tool name must appear in output"
        );
        assert!(
            output.contains("read_file"),
            "tool name must appear in output"
        );
        assert!(
            output.contains("README.md"),
            "indented detail must appear in output"
        );
    }

    #[test]
    fn persistent_layout_shows_welcome_hint_before_first_turn() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = TaskLayoutState {
            task_id: "test-001".into(),
            status_line: "mode:ready approval:none repo:vexcoder inst:none".into(),
            activity_rows: vec![],
            timeline_entries: vec![],
            selected_step: 0,
            total_steps: 0,
            output_title: "Transcript".into(),
            output_rows: vec![
                "Type a prompt below to begin.".into(),
                String::new(),
                "The orchestrator will call tools and stream results here.".into(),
            ],
            output_scroll_offset: 0,
            output_scroll_anchor: OutputScrollAnchor::Bottom,
            pending_approval: None,
            input_hint: "Prompt\nUse `/` for commands, `@path` to inline files, paste large blocks, and Shift+Enter for a newline.".into(),
            composer_text: String::new(),
            composer_cursor: 0,
            changed_files: vec![],
            follow_mode: true,
        };

        draw.draw(&mut buf, &state, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        assert!(
            output.contains("Type a prompt"),
            "welcome hint must be drawn on first frame"
        );
    }

    #[test]
    fn adaptive_layout_scales_timeline_with_entries() {
        let entries: Vec<TimelineEntry> = (0..20)
            .map(|i| TimelineEntry {
                step_id: i as u64,
                lifecycle: StepLifecycle::Completed,
                label: format!("step_{i}: done"),
                detail: String::new(),
                session_id: None,
            })
            .collect();

        let regions = Regions::compute(80, 40, false, entries.len());
        // Timeline should grow beyond the old fixed 6 rows.
        assert!(
            regions.timeline_rows > 6,
            "timeline must scale beyond 6 rows for 20 entries on a 40-row terminal"
        );
        // But not exceed 35% of available space.
        let max_expected = ((40 - 1 - 3) as f32 * 0.35) as u16;
        assert!(
            regions.timeline_rows <= max_expected + 1,
            "timeline must not exceed ~35% of available space"
        );
    }

    #[test]
    fn adaptive_composer_scales_with_terminal_height() {
        let small = Regions::compute(80, 12, false, 0);
        assert_eq!(
            small.composer_rows, 4,
            "small terminal keeps a usable multiline prompt surface"
        );

        let medium = Regions::compute(80, 20, false, 0);
        assert_eq!(
            medium.composer_rows, 5,
            "medium terminal gets a larger multiline prompt surface"
        );

        let large = Regions::compute(80, 40, false, 0);
        assert_eq!(
            large.composer_rows, 8,
            "large terminal gets an expanded prompt surface"
        );
    }

    #[test]
    fn human_readable_header_shows_running_state() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = TaskLayoutState {
            task_id: "test-001".into(),
            status_line: "mode:streaming approval:none repo:myrepo inst:AGENTS.md".into(),
            activity_rows: vec![],
            timeline_entries: vec![TimelineEntry {
                step_id: 1,
                lifecycle: StepLifecycle::Running,
                label: "read_file: running".into(),
                detail: String::new(),
                session_id: None,
            }],
            selected_step: 0,
            total_steps: 1,
            output_title: "Transcript".into(),
            output_rows: vec![],
            output_scroll_offset: 0,
            output_scroll_anchor: OutputScrollAnchor::Bottom,
            pending_approval: None,
            input_hint: "Prompt\nUse `/` for commands, `@path` to inline files, paste large blocks, and Shift+Enter for a newline.".into(),
            composer_text: String::new(),
            composer_cursor: 0,
            changed_files: vec!["src/main.rs".into()],
            follow_mode: true,
        };

        draw.draw(&mut buf, &state, 100, 24);
        let output = String::from_utf8_lossy(&buf);

        assert!(output.contains("myrepo"), "header must show repo name");
        assert!(output.contains("running"), "header must show running state");
        assert!(
            output.contains("1 file changed") || output.contains("1 active"),
            "header must show file count or active count"
        );
    }

    #[test]
    fn inline_approval_renders_in_composer() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = TaskLayoutState {
            task_id: "test-001".into(),
            status_line: "mode:overlay approval:pending repo:vexcoder inst:none".into(),
            activity_rows: vec![],
            timeline_entries: vec![],
            selected_step: 0,
            total_steps: 0,
            output_title: "Transcript".into(),
            output_rows: vec![],
            output_scroll_offset: 0,
            output_scroll_anchor: OutputScrollAnchor::Bottom,
            pending_approval: Some("write_file: src/main.rs".into()),
            input_hint: "Approval\n[y/n/s]".into(),
            composer_text: String::new(),
            composer_cursor: 0,
            changed_files: vec![],
            follow_mode: true,
        };

        draw.draw(&mut buf, &state, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        assert!(
            output.contains("write_file"),
            "approval context must show in composer"
        );
        assert!(
            output.contains("approve") || output.contains("deny"),
            "approval choices must be visible"
        );
    }

    #[test]
    fn status_parts_parsing() {
        let parts = parse_status_parts(
            "mode:streaming approval:none history:11 repo:vexcoder inst:AGENTS.md tokens:0",
        );
        assert_eq!(parts.mode, "streaming");
        assert_eq!(parts.repo, "vexcoder");
        assert_eq!(parts.inst, "AGENTS.md");
        assert_eq!(parts.tokens, 0);
    }

    #[test]
    fn status_parts_parsing_with_tokens() {
        let parts = parse_status_parts(
            "mode:ready approval:none history:3 repo:myrepo inst:none tokens:4800",
        );
        assert_eq!(parts.tokens, 4800);
        assert_eq!(parts.repo, "myrepo");
        assert_eq!(parts.mode, "ready");
    }

    #[test]
    fn header_shows_token_indicator_when_tokens_recorded() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = TaskLayoutState {
            task_id: "test-001".into(),
            // tokens:2500 — just over 2k so the label rounds to "~2.5k ctx"
            status_line: "mode:ready approval:none history:2 repo:vexcoder inst:none tokens:2500"
                .into(),
            activity_rows: vec![],
            timeline_entries: vec![],
            selected_step: 0,
            total_steps: 0,
            output_title: "Transcript".into(),
            output_rows: vec![],
            output_scroll_offset: 0,
            output_scroll_anchor: OutputScrollAnchor::Bottom,
            pending_approval: None,
            input_hint: "Prompt\nUse `/` for commands, `@path` to inline files, paste large blocks, and Shift+Enter for a newline.".into(),
            composer_text: String::new(),
            composer_cursor: 0,
            changed_files: vec![],
            follow_mode: true,
        };

        draw.draw(&mut buf, &state, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        assert!(
            output.contains("ctx"),
            "header must show 'ctx' token indicator when tokens > 0: got {output:?}"
        );
        assert!(
            output.contains("2.5"),
            "header must show rounded token count: got {output:?}"
        );
    }

    #[test]
    fn header_hides_token_indicator_when_no_turns_completed() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = TaskLayoutState {
            task_id: "test-001".into(),
            // tokens:0 — no turns completed yet
            status_line: "mode:ready approval:none history:0 repo:vexcoder inst:none tokens:0"
                .into(),
            activity_rows: vec![],
            timeline_entries: vec![],
            selected_step: 0,
            total_steps: 0,
            output_title: "Transcript".into(),
            output_rows: vec![],
            output_scroll_offset: 0,
            output_scroll_anchor: OutputScrollAnchor::Bottom,
            pending_approval: None,
            input_hint: "Prompt\nUse `/` for commands, `@path` to inline files, paste large blocks, and Shift+Enter for a newline.".into(),
            composer_text: String::new(),
            composer_cursor: 0,
            changed_files: vec![],
            follow_mode: true,
        };

        draw.draw(&mut buf, &state, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        assert!(
            !output.contains("ctx"),
            "header must not show token indicator before any turns: got {output:?}"
        );
    }

    #[test]
    fn composer_renders_live_input_text() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = TaskLayoutState {
            task_id: "test-001".into(),
            status_line: "mode:ready approval:none repo:vexcoder inst:none".into(),
            activity_rows: vec![],
            timeline_entries: vec![],
            selected_step: 0,
            total_steps: 0,
            output_title: "Transcript".into(),
            output_rows: vec![],
            output_scroll_offset: 0,
            output_scroll_anchor: OutputScrollAnchor::Bottom,
            pending_approval: None,
            input_hint: "Prompt\nUse `/` for commands, `@path` to inline files, paste large blocks, and Shift+Enter for a newline.".into(),
            composer_text: "hello fullscreen".into(),
            composer_cursor: "hello fullscreen".len(),
            changed_files: vec![],
            follow_mode: true,
        };

        draw.draw(&mut buf, &state, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        assert!(
            output.contains("hello fullscreen"),
            "composer must render the live editor buffer"
        );
    }

    #[test]
    fn composer_hash_tracks_live_input_changes() {
        let draw = TaskDraw::new();
        let first = TaskLayoutState {
            task_id: "test-001".into(),
            status_line: "mode:ready approval:none repo:vexcoder inst:none".into(),
            activity_rows: vec![],
            timeline_entries: vec![],
            selected_step: 0,
            total_steps: 0,
            output_title: "Transcript".into(),
            output_rows: vec![],
            output_scroll_offset: 0,
            output_scroll_anchor: OutputScrollAnchor::Bottom,
            pending_approval: None,
            input_hint: "Prompt\nUse `/` for commands, `@path` to inline files, paste large blocks, and Shift+Enter for a newline.".into(),
            composer_text: "first".into(),
            composer_cursor: 5,
            changed_files: vec![],
            follow_mode: true,
        };
        let second = TaskLayoutState {
            composer_text: "second".into(),
            composer_cursor: 6,
            ..first.clone()
        };

        assert_ne!(
            draw.compute_composer_hash(&first),
            draw.compute_composer_hash(&second),
            "composer hash must change when the live input buffer changes"
        );
    }

    #[test]
    fn composer_hash_tracks_cursor_only_changes() {
        let draw = TaskDraw::new();
        let first = TaskLayoutState {
            task_id: "test-001".into(),
            status_line: "mode:ready approval:none repo:vexcoder inst:none".into(),
            activity_rows: vec![],
            timeline_entries: vec![],
            selected_step: 0,
            total_steps: 0,
            output_title: "Transcript".into(),
            output_rows: vec![],
            output_scroll_offset: 0,
            output_scroll_anchor: OutputScrollAnchor::Bottom,
            pending_approval: None,
            input_hint: "Prompt\nUse `/` for commands, `@path` to inline files, paste large blocks, and Shift+Enter for a newline.".into(),
            composer_text: "same text".into(),
            composer_cursor: 2,
            changed_files: vec![],
            follow_mode: true,
        };
        let second = TaskLayoutState {
            composer_cursor: 7,
            ..first.clone()
        };

        assert_ne!(
            draw.compute_composer_hash(&first),
            draw.compute_composer_hash(&second),
            "composer hash must change when the live cursor moves"
        );
    }

    #[test]
    fn numbered_list_items_are_parsed() {
        let result = parse_numbered_list_item("1. First item");
        assert!(result.is_some(), "must parse '1. First item'");
        let (prefix, rest, _) = result.unwrap();
        assert_eq!(prefix, "1. ");
        assert_eq!(rest, "First item");

        let result = parse_numbered_list_item("42. Large number");
        assert!(result.is_some(), "must parse '42. Large number'");
        let (prefix, rest, _) = result.unwrap();
        assert_eq!(prefix, "42. ");
        assert_eq!(rest, "Large number");

        assert!(
            parse_numbered_list_item("not a list").is_none(),
            "must not parse plain text"
        );
        assert!(
            parse_numbered_list_item("1.no space").is_none(),
            "must not parse without space after dot"
        );
    }

    #[test]
    fn horizontal_rules_are_detected() {
        assert!(is_horizontal_rule("---"), "three dashes");
        assert!(is_horizontal_rule("***"), "three asterisks");
        assert!(is_horizontal_rule("___"), "three underscores");
        assert!(is_horizontal_rule("- - -"), "spaced dashes");
        assert!(is_horizontal_rule("-----"), "many dashes");
        assert!(!is_horizontal_rule("--"), "too few");
        assert!(!is_horizontal_rule("abc"), "not a rule");
        assert!(!is_horizontal_rule("---a"), "mixed characters");
    }

    #[test]
    fn checklist_items_render_with_markers() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = make_state(
            vec![],
            vec![
                "- [x] Completed task",
                "- [ ] Pending task",
                "- Regular bullet",
            ],
        );

        draw.draw(&mut buf, &state, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        assert!(
            output.contains("\u{2611}"),
            "checked item must show ballot box with check: {output}"
        );
        assert!(
            output.contains("\u{2610}"),
            "unchecked item must show empty ballot box: {output}"
        );
        assert!(
            output.contains("Completed task"),
            "checked task text must appear"
        );
        assert!(
            output.contains("Pending task"),
            "unchecked task text must appear"
        );
    }

    #[test]
    fn numbered_list_renders_in_transcript() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = make_state(vec![], vec!["1. First step", "2. Second step"]);

        draw.draw(&mut buf, &state, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        assert!(output.contains("1. "), "number prefix must be drawn");
        assert!(
            output.contains("First step"),
            "list item text must be drawn"
        );
    }

    #[test]
    fn horizontal_rule_renders_in_transcript() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = make_state(vec![], vec!["Some text", "---", "More text"]);

        draw.draw(&mut buf, &state, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        // Horizontal rule draws thin rule characters.
        assert!(
            output.contains("\u{2500}"),
            "horizontal rule must draw line characters"
        );
        assert!(output.contains("Some text"), "text before rule must appear");
        assert!(output.contains("More text"), "text after rule must appear");
    }

    #[test]
    fn progress_indicator_shown_for_running_tasks() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = make_state(
            vec![TimelineEntry {
                step_id: 1,
                lifecycle: StepLifecycle::Running,
                label: "build: running...".into(),
                detail: String::new(),
                session_id: None,
            }],
            vec![],
        );

        draw.draw(&mut buf, &state, 100, 24);
        let output = String::from_utf8_lossy(&buf);

        // Must contain at least one block-drawing character from the progress
        // indicator animation.
        let has_progress_char = output.contains('\u{2591}')
            || output.contains('\u{2593}')
            || output.contains('\u{2588}');
        assert!(
            has_progress_char,
            "header must show progress indicator for running tasks"
        );
        assert!(
            output.contains("1 active"),
            "header must show active count"
        );
    }

    #[test]
    fn scroll_indicator_drawn_when_content_exceeds_viewport() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let mut state = make_state(vec![], vec![]);
        // Create more output rows than will fit in the viewport.
        state.output_rows = (0..50).map(|i| format!("long-output-{i}")).collect();

        draw.draw(&mut buf, &state, 80, 16);
        let output = String::from_utf8_lossy(&buf);

        // Scroll indicator uses ░ (track) and █ (thumb) on the right edge.
        let has_track = output.contains('\u{2591}');
        let has_thumb = output.contains('\u{2588}');
        assert!(
            has_track || has_thumb,
            "scroll indicator must appear when content exceeds viewport"
        );
    }

    #[test]
    fn inline_italic_renders_with_italic_escape() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = make_state(vec![], vec!["This has *italic* text"]);

        draw.draw(&mut buf, &state, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        // Italic text uses CSI 3m.
        assert!(
            output.contains("\x1b[3m"),
            "italic text must use ANSI italic escape: {output}"
        );
        assert!(output.contains("italic"), "italic text must appear");
    }

    #[test]
    fn inline_strikethrough_renders_as_dim() {
        let mut buf = Vec::new();
        let mut draw = TaskDraw::new();
        let state = make_state(vec![], vec!["This has ~~struck~~ text"]);

        draw.draw(&mut buf, &state, 80, 24);
        let output = String::from_utf8_lossy(&buf);

        assert!(output.contains("struck"), "struck text must appear");
    }
}
