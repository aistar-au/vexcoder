// ── Adaptive region geometry ────────────────────────────────────────

/// Adaptive layout regions that scale with terminal dimensions.
///
/// ```text
/// row 0        ┌─ header (repo + status) ───────┐  (1 row)
/// row 1        ├─ changed files (optional) ──────┤  (0..1 rows)
/// row H..T     ├─ timeline (adaptive height) ────┤  (3..35% of rows)
/// row T..C     │  transcript (remaining rows)    │  (fills remaining)
/// row C..S     ├─ composer (adaptive) ───────────┤  (3..8 rows)
/// row S        ├─ status bar (key hints + task)  ┤  (1 row)
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
    pub(super) fn compute(
        cols: u16,
        rows: u16,
        has_files: bool,
        timeline_entry_count: usize,
    ) -> Self {
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
