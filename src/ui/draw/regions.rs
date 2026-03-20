use crate::ui::layout::preferred_four_region_input_rows;

// ── Adaptive region geometry ────────────────────────────────────────

/// Adaptive layout regions that scale with terminal dimensions.
///
/// ```text
/// row 0..C     │  transcript (remaining rows)    │  (fills remaining)
/// row C..end   ├─ composer (fixed) ──────────────┤  (3 rows)
///              └─────────────────────────────────┘
/// ```
pub(super) struct Regions {
    pub(super) cols: u16,
    pub(super) rows: u16,
    pub(super) files_row: Option<u16>,
    pub(super) timeline_start: u16,
    pub(super) timeline_rows: u16,
    pub(super) transcript_start: u16,
    pub(super) transcript_rows: u16,
    pub(super) composer_start: u16,
    pub(super) composer_rows: u16,
    pub(super) status_bar_row: u16,
}

/// Minimum transcript rows reserved above the prompt surface.
const MIN_TRANSCRIPT_ROWS: u16 = 2;
/// Fullscreen prompt rows (label + two live rows).
const FIXED_COMPOSER_ROWS: u16 = 3;

impl Regions {
    pub(super) fn compute(
        cols: u16,
        rows: u16,
        has_files: bool,
        timeline_entry_count: usize,
    ) -> Self {
        let files_row = None;

        // Reserve 1 row for status bar at the very bottom.
        let status_bar_row = rows.saturating_sub(1);

        // The fullscreen prompt stays fixed at three rows unless the
        // terminal is too short to preserve transcript rows above it.
        let _ = has_files;
        let available = rows.saturating_sub(1);
        let max_composer_rows = available.saturating_sub(MIN_TRANSCRIPT_ROWS).max(1);
        let composer_rows = preferred_four_region_input_rows(rows)
            .min(FIXED_COMPOSER_ROWS)
            .min(max_composer_rows)
            .max(1);

        let available = available.saturating_sub(composer_rows);

        // The fullscreen ANSI surface does not reserve a dedicated top
        // timeline strip. The transcript owns the full body above the
        // composer and renders tool/state paragraphs directly.
        let _ = timeline_entry_count;
        let timeline_rows = 0;
        let transcript_rows = available;

        let timeline_start = 0;
        let transcript_start = 0;
        let composer_start = transcript_start + transcript_rows;

        Regions {
            cols,
            rows,
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
