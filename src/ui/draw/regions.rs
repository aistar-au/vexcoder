use crate::ui::input_metrics::visual_row_count;
use crate::ui::layout::MAX_INPUT_PANE_ROWS;

// ── Adaptive region geometry ────────────────────────────────────────

/// Adaptive layout regions for the operator CLI surface.
///
/// Layout (top to bottom):
///
/// ```text
/// row 0..T     │ Transcript (scrollable, fills remaining)    │
/// row T..B     │ Telemetry pane  │  Git pane (fixed height)  │
/// row B..C     │ Composer / prompt area (adaptive, persistent)│
/// row C        │ Status bar                                   │
/// ```
///
/// The transcript region fills the top of the screen and scrolls
/// indefinitely upward consistent with ADR-031. Tool call activity
/// and enriched responses render inline in the transcript with
/// ADR-040 telemetry counters. The two non-scrolling bottom panes
/// show telemetry and git status side by side. The composer sits
/// at the bottom so the operator always has persistent prompt access.
pub(super) struct Regions {
    pub(super) cols: u16,
    pub(super) rows: u16,
    /// First row of the scrollable transcript area (always 0).
    pub(super) transcript_start: u16,
    pub(super) transcript_rows: u16,
    /// First row of the telemetry + git bottom panes.
    pub(super) bottom_pane_start: u16,
    pub(super) bottom_pane_rows: u16,
    /// Column at which the bottom pane splits (telemetry left, git right).
    pub(super) bottom_pane_split_col: u16,
    /// First row of the composer (prompt) area.
    pub(super) composer_start: u16,
    pub(super) composer_rows: u16,
    pub(super) status_bar_row: u16,
}

/// Minimum transcript rows reserved for the scrolling area.
const MIN_TRANSCRIPT_ROWS: u16 = 2;
/// Fullscreen prompt rows always reserve a label row and at least two body rows.
const MIN_COMPOSER_ROWS: u16 = 3;
/// Fixed height for the bottom telemetry + git panes.
const BOTTOM_PANE_ROWS: u16 = 4;

impl Regions {
    #[cfg(test)]
    pub(super) fn compute(
        cols: u16,
        rows: u16,
        has_files: bool,
        timeline_entry_count: usize,
    ) -> Self {
        Self::compute_with_composer(cols, rows, has_files, timeline_entry_count, "")
    }

    pub(super) fn compute_with_composer(
        cols: u16,
        rows: u16,
        has_files: bool,
        timeline_entry_count: usize,
        composer_text: &str,
    ) -> Self {
        let _ = has_files;
        let _ = timeline_entry_count;

        // Reserve 1 row for status bar at the very bottom.
        let status_bar_row = rows.saturating_sub(1);
        let available = rows.saturating_sub(1); // above status bar

        // Composer (prompt) — sits just above the status bar, adaptive height.
        let input_width = cols.saturating_sub(2).max(1) as usize;
        let desired_input_rows =
            visual_row_count(composer_text, input_width).clamp(1, MAX_INPUT_PANE_ROWS) as u16;
        let desired_composer_rows = desired_input_rows.max(2).saturating_add(1);
        let max_composer_rows = available
            .saturating_sub(MIN_TRANSCRIPT_ROWS + BOTTOM_PANE_ROWS)
            .max(1);
        let composer_rows = desired_composer_rows
            .min(max_composer_rows)
            .max(MIN_COMPOSER_ROWS.min(max_composer_rows));
        let composer_start = status_bar_row.saturating_sub(composer_rows);

        // Bottom panes (telemetry + git) — fixed height, above composer.
        let above_composer = composer_start;
        let bottom_pane_rows =
            BOTTOM_PANE_ROWS.min(above_composer.saturating_sub(MIN_TRANSCRIPT_ROWS));
        let bottom_pane_start = composer_start.saturating_sub(bottom_pane_rows);
        let bottom_pane_split_col = cols / 2;

        // Transcript — fills the top, everything above the bottom panes.
        let transcript_start = 0;
        let transcript_rows = bottom_pane_start;

        Regions {
            cols,
            rows,
            transcript_start,
            transcript_rows,
            bottom_pane_start,
            bottom_pane_rows,
            bottom_pane_split_col,
            composer_start,
            composer_rows,
            status_bar_row,
        }
    }
}
