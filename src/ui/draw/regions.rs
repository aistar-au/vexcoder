use crate::ui::layout::preferred_four_region_input_rows;

// ── Adaptive region geometry ────────────────────────────────────────

/// Adaptive layout regions that scale with terminal dimensions.
///
/// ```text
/// row 0        ┌─ header (repo + status) ───────┐  (1 row)
/// row 1        ├─ changed files (optional) ──────┤  (0..1 rows)
/// row H..C     │  transcript (full body area)    │  (fills remaining)
/// row C..end   ├─ composer (adaptive) ───────────┤  (1..8 rows)
///              └─────────────────────────────────┘
/// ```
pub(super) struct Regions {
    pub(super) cols: u16,
    pub(super) rows: u16,
    pub(super) header_row: u16,
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
/// Preferred minimum fullscreen prompt rows (toolbar + multiline input).
const MIN_COMPOSER_ROWS: u16 = 3;

impl Regions {
    pub(super) fn compute(
        cols: u16,
        rows: u16,
        has_files: bool,
        _timeline_entry_count: usize,
    ) -> Self {
        let header_row = 0;
        let files_row = if has_files { Some(1) } else { None };
        let header_height = if has_files { 2u16 } else { 1u16 };

        // Reserve 1 row for status bar at the very bottom.
        let status_bar_row = rows.saturating_sub(1);

        // Composer: dedicate a larger bottom-docked prompt surface while
        // preserving room for the transcript whenever the terminal allows it.
        let available = rows.saturating_sub(header_height).saturating_sub(1);
        let preferred_composer_rows = preferred_four_region_input_rows(rows).max(MIN_COMPOSER_ROWS);
        let max_composer_rows = if available == 0 {
            0
        } else {
            available.saturating_sub(MIN_TRANSCRIPT_ROWS).max(1)
        };
        let composer_rows = preferred_composer_rows.min(max_composer_rows).min(available);

        // The ANSI fullscreen surface now lets the transcript own the whole
        // body area above the composer instead of reserving a dedicated
        // timeline/activity pane.
        let timeline_rows = 0;
        let transcript_rows = available.saturating_sub(composer_rows);

        let timeline_start = header_height;
        let transcript_start = header_height;
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
