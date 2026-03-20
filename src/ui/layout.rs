use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub(crate) const MAX_TIMELINE_FRACTION: f32 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreePaneLayout {
    pub header: Rect,
    pub history: Rect,
    pub input: Rect,
}

pub fn split_three_pane_layout(area: Rect, input_rows: u16) -> ThreePaneLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(input_rows.max(1)),
        ])
        .split(area);

    ThreePaneLayout {
        header: chunks[0],
        history: chunks[1],
        input: chunks[2],
    }
}

/// Four-region layout: header, activity trail, output pane, input pane
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FourRegionLayout {
    pub header: Rect,
    pub activity: Rect,
    pub output: Rect,
    pub input: Rect,
}

pub(crate) fn preferred_four_region_input_rows(rows: u16) -> u16 {
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

pub fn split_four_region_layout(area: Rect, header_rows: u16, input_rows: u16) -> FourRegionLayout {
    let header_rows = header_rows.clamp(1, 2).min(area.height);
    let max_input_rows = area
        .height
        .saturating_sub(header_rows)
        .saturating_sub(1)
        .max(1);
    let input_rows = input_rows.clamp(3, 8).min(max_input_rows);
    let body_rows = area
        .height
        .saturating_sub(header_rows)
        .saturating_sub(input_rows);
    let activity_rows = if body_rows <= 1 {
        body_rows
    } else if body_rows <= 3 {
        body_rows.saturating_sub(1)
    } else {
        (((body_rows as f32) * MAX_TIMELINE_FRACTION) as u16).clamp(3, body_rows.saturating_sub(1))
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_rows),
            Constraint::Length(activity_rows),
            Constraint::Min(1),
            Constraint::Length(input_rows),
        ])
        .split(area);

    FourRegionLayout {
        header: chunks[0],
        activity: chunks[1],
        output: chunks[2],
        input: chunks[3],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_splits_into_three_panes() {
        let area = Rect::new(0, 0, 80, 20);
        let panes = split_three_pane_layout(area, 4);

        assert_eq!(panes.header.height, 1);
        assert_eq!(panes.history.height, 15);
        assert_eq!(panes.input.height, 4);
        assert_eq!(panes.header.y, 0);
        assert_eq!(panes.history.y, 1);
        assert_eq!(panes.input.y, 16);
    }

    #[test]
    fn layout_preserves_dynamic_input_height() {
        let area = Rect::new(0, 0, 80, 12);
        let panes = split_three_pane_layout(area, 6);

        assert_eq!(panes.input.height, 6);
        assert_eq!(panes.header.height, 1);
        assert_eq!(panes.history.height, 5);
    }

    #[test]
    fn test_task_layout_four_regions_render_without_panic() {
        let area = Rect::new(0, 0, 80, 24);
        let layout = split_four_region_layout(area, 1, 3);

        // Verify four regions are created with expected constraints
        assert_eq!(layout.header.height, 1);
        assert!(layout.activity.height > 0);
        assert_eq!(layout.input.height, 3);
        assert!(layout.output.height > 0);

        // Verify vertical stacking
        assert_eq!(layout.header.y, 0);
        assert_eq!(layout.activity.y, 1);
        assert_eq!(layout.output.y, 1 + layout.activity.height);
        assert_eq!(layout.input.y, layout.output.y + layout.output.height);
    }

    #[test]
    fn preferred_input_rows_match_adaptive_fullscreen_tiers() {
        assert_eq!(preferred_four_region_input_rows(12), 4);
        assert_eq!(preferred_four_region_input_rows(20), 5);
        assert_eq!(preferred_four_region_input_rows(24), 6);
        assert_eq!(preferred_four_region_input_rows(30), 7);
        assert_eq!(preferred_four_region_input_rows(40), 8);
    }

    #[test]
    fn four_region_layout_scales_activity_beyond_six_rows_on_tall_terminals() {
        let area = Rect::new(0, 0, 80, 40);
        let layout = split_four_region_layout(area, 1, 8);

        assert!(
            layout.activity.height > 6,
            "activity pane should grow beyond the earlier 6-row window on tall terminals"
        );
        assert!(layout.output.height > layout.activity.height);
    }
}
