use crate::ui::tui::layout::{Constraint, Direction, Layout};

pub use crate::ui::tui::layout::Rect;

pub const MAX_INPUT_PANE_ROWS: usize = 6;
pub(crate) const MIN_FULLSCREEN_INPUT_ROWS: u16 = 3;

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
            Constraint::Length(0),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FourRegionLayout {
    pub header: Rect,
    pub activity: Rect,
    pub output: Rect,
    pub input: Rect,
}

#[cfg(test)]
pub(crate) fn preferred_four_region_input_rows(_rows: u16) -> u16 {
    preferred_four_region_input_rows_for_content(_rows, 1)
}

pub(crate) fn preferred_four_region_input_rows_for_content(_rows: u16, desired_rows: u16) -> u16 {
    desired_rows.clamp(MIN_FULLSCREEN_INPUT_ROWS, MAX_INPUT_PANE_ROWS as u16)
}

pub fn split_four_region_layout(area: Rect, header_rows: u16, input_rows: u16) -> FourRegionLayout {
    let header_rows = header_rows.min(1).min(area.height);
    let available_body = area.height.saturating_sub(header_rows);
    let max_input_rows = available_body.saturating_sub(1).max(1);
    let input_rows = input_rows
        .clamp(1, MAX_INPUT_PANE_ROWS as u16)
        .min(max_input_rows);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_rows),
            Constraint::Length(0),
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

pub(crate) fn split_compact_task_layout(
    area: Rect,
    header_rows: u16,
    output_rows: u16,
    input_rows: u16,
) -> FourRegionLayout {
    let header_rows = header_rows.min(area.height);
    let remaining = area.height.saturating_sub(header_rows);
    let capped_input = input_rows
        .clamp(1, MAX_INPUT_PANE_ROWS as u16)
        .min(remaining);
    let available_for_output = remaining.saturating_sub(capped_input);
    let capped_output = output_rows.min(available_for_output);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_rows),
            Constraint::Length(0),
            Constraint::Length(capped_output),
            Constraint::Length(capped_input),
            Constraint::Min(0),
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

        assert_eq!(panes.header.height, 0);
        assert_eq!(panes.history.height, 16);
        assert_eq!(panes.input.height, 4);
        assert_eq!(panes.header.y, 0);
        assert_eq!(panes.history.y, 0);
        assert_eq!(panes.input.y, 16);
    }

    #[test]
    fn layout_preserves_dynamic_input_height() {
        let area = Rect::new(0, 0, 80, 12);
        let panes = split_three_pane_layout(area, 6);

        assert_eq!(panes.input.height, 6);
        assert_eq!(panes.header.height, 0);
        assert_eq!(panes.history.height, 6);
    }

    #[test]
    fn test_task_layout_four_regions_render_without_panic() {
        let area = Rect::new(0, 0, 80, 24);
        let layout = split_four_region_layout(area, 0, 3);

        assert_eq!(layout.header.height, 0);
        assert_eq!(layout.activity.height, 0);
        assert_eq!(layout.input.height, 3);
        assert!(layout.output.height > 0);

        assert_eq!(layout.header.y, 0);
        assert_eq!(layout.activity.y, 0);
        assert_eq!(layout.output.y, layout.activity.y + layout.activity.height);
        assert_eq!(layout.input.y, layout.output.y + layout.output.height);
    }

    #[test]
    fn preferred_input_rows_remain_fixed_to_three_lines() {
        assert_eq!(preferred_four_region_input_rows(12), 3);
        assert_eq!(preferred_four_region_input_rows(20), 3);
        assert_eq!(preferred_four_region_input_rows(24), 3);
        assert_eq!(preferred_four_region_input_rows(30), 3);
        assert_eq!(preferred_four_region_input_rows(40), 3);
    }

    #[test]
    fn preferred_input_rows_expand_for_wrapped_content() {
        assert_eq!(preferred_four_region_input_rows_for_content(24, 1), 3);
        assert_eq!(preferred_four_region_input_rows_for_content(24, 4), 4);
        assert_eq!(preferred_four_region_input_rows_for_content(24, 9), 6);
    }

    #[test]
    fn four_region_layout_keeps_full_body_for_transcript_on_tall_terminals() {
        let area = Rect::new(0, 0, 80, 40);
        let layout = split_four_region_layout(area, 0, 3);

        assert_eq!(layout.activity.height, 0);
        assert_eq!(layout.input.height, 3);
        assert!(layout.output.height >= 36);
    }

    #[test]
    fn compact_task_layout_places_composer_below_content() {
        let area = Rect::new(0, 0, 80, 20);
        let layout = split_compact_task_layout(area, 1, 2, 3);

        assert_eq!(layout.header.y, 0);
        assert_eq!(layout.header.height, 1);

        assert_eq!(layout.output.y, 1);
        assert_eq!(layout.output.height, 2);

        assert_eq!(layout.input.y, 3);
        assert_eq!(layout.input.height, 3);
    }

    #[test]
    fn compact_task_layout_fills_viewport_when_content_is_large() {
        let area = Rect::new(0, 0, 80, 20);
        let layout = split_compact_task_layout(area, 1, 16, 3);

        assert_eq!(layout.output.height, 16);
        assert_eq!(layout.input.y, layout.output.y + layout.output.height);
    }

    #[test]
    fn compact_task_layout_zero_output_rows() {
        let area = Rect::new(0, 0, 80, 20);
        let layout = split_compact_task_layout(area, 1, 0, 3);

        assert_eq!(layout.output.height, 0);

        assert_eq!(layout.input.y, 1);
    }
}
