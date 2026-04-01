use crate::ui::input_metrics::visual_row_count;
use crate::ui::layout::MAX_INPUT_PANE_ROWS;

// ── Adaptive region geometry ────────────────────────────────────────

/// Adaptive layout regions that scale with display dimensions.
///
/// ```text
/// row 0..T     │  transcript (scrollable)        │  (fills top)
/// row T..B     ├─ telemetry/git (fixed) ─────────┤
/// row B..C     ├─ composer (adaptive) ───────────┤
/// row end-1    └─ status bar ────────────────────┘
/// ```
pub(super) struct Regions {
    pub(super) cols: u16,
    pub(super) rows: u16,
    pub(super) transcript_start: u16,
    pub(super) transcript_rows: u16,
    pub(super) bottom_pane_start: u16,
    pub(super) bottom_pane_rows: u16,
    pub(super) bottom_pane_split_col: u16,
    pub(super) composer_start: u16,
    pub(super) composer_rows: u16,
    pub(super) status_bar_row: u16,
}

/// Minimum transcript rows reserved above the prompt surface.
const MIN_TRANSCRIPT_ROWS: u16 = 2;
/// Fullscreen prompt rows always reserve a label row and at least two body rows.
const MIN_COMPOSER_ROWS: u16 = 3;
/// The non-scrolling telemetry/git pane keeps a stable 4-row height when space allows.
const FIXED_BOTTOM_PANE_ROWS: u16 = 4;

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
        // Reserve 1 row for status bar at the very bottom.
        let status_bar_row = rows.saturating_sub(1);

        // The fullscreen prompt grows with wrapped content up to the shared
        // input cap, while preserving transcript rows and the fixed bottom pane
        // above it.
        let _ = has_files;
        let available = rows.saturating_sub(1);
        let input_width = cols.saturating_sub(2).max(1) as usize;
        let desired_input_rows =
            visual_row_count(composer_text, input_width).clamp(1, MAX_INPUT_PANE_ROWS) as u16;
        let desired_composer_rows = desired_input_rows.max(2).saturating_add(1);
        let max_composer_rows = available
            .saturating_sub(MIN_TRANSCRIPT_ROWS.saturating_add(FIXED_BOTTOM_PANE_ROWS))
            .max(1);
        let composer_rows = desired_composer_rows
            .min(max_composer_rows)
            .max(MIN_COMPOSER_ROWS.min(max_composer_rows));

        let remaining = available.saturating_sub(composer_rows);
        let bottom_pane_rows = remaining
            .saturating_sub(MIN_TRANSCRIPT_ROWS)
            .min(FIXED_BOTTOM_PANE_ROWS);

        // The fullscreen ANSI surface renders task-state and orchestrator
        // paragraphs in the transcript, keeping only the telemetry/git split
        // pane fixed above the composer.
        let _ = timeline_entry_count;
        let transcript_rows = remaining.saturating_sub(bottom_pane_rows);

        let transcript_start = 0;
        let bottom_pane_start = transcript_start + transcript_rows;
        let composer_start = bottom_pane_start + bottom_pane_rows;
        let max_split = cols.saturating_sub(2).max(1);
        let bottom_pane_split_col = cols.saturating_div(2).max(1).min(max_split);

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
