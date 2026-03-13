use ratatui::layout::{Constraint, Direction, Layout, Rect};

const MAX_OVERLAY_HISTORY_ROWS: u16 = 12;
const MAX_OVERLAY_INPUT_ROWS: u16 = 6;
const MAX_TASK_OVERLAY_ROWS: u16 = 18;

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

pub fn bottom_overlay_area(area: Rect, overlay_rows: u16) -> Rect {
    let overlay_rows = overlay_rows.min(area.height);
    let overlay_y = area
        .y
        .saturating_add(area.height.saturating_sub(overlay_rows));
    Rect::new(area.x, overlay_y, area.width, overlay_rows)
}

pub fn split_overlay_three_pane_layout(area: Rect, input_rows: u16) -> ThreePaneLayout {
    if area.height == 0 || area.width == 0 {
        return ThreePaneLayout {
            header: area,
            history: area,
            input: area,
        };
    }

    let max_overlay_rows = 1 + MAX_OVERLAY_HISTORY_ROWS + MAX_OVERLAY_INPUT_ROWS;
    let overlay = bottom_overlay_area(area, max_overlay_rows);
    let body_rows = overlay.height.saturating_sub(1);

    let input_rows = match body_rows {
        0 => 0,
        1 => 1,
        _ => input_rows
            .clamp(1, body_rows - 1)
            .min(MAX_OVERLAY_INPUT_ROWS),
    };
    let history_rows = body_rows.saturating_sub(input_rows);

    let header = Rect::new(overlay.x, overlay.y, overlay.width, overlay.height.min(1));
    let history = Rect::new(
        overlay.x,
        header.y.saturating_add(header.height),
        overlay.width,
        history_rows,
    );
    let input = Rect::new(
        overlay.x,
        history.y.saturating_add(history.height),
        overlay.width,
        input_rows,
    );

    ThreePaneLayout {
        header,
        history,
        input,
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

pub fn split_four_region_layout(area: Rect, header_rows: u16, input_rows: u16) -> FourRegionLayout {
    let header_rows = header_rows.clamp(1, 2);
    let input_rows = input_rows.clamp(3, 4);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_rows),
            Constraint::Percentage(28),
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

pub fn split_overlay_four_region_layout(
    area: Rect,
    header_rows: u16,
    input_rows: u16,
) -> FourRegionLayout {
    split_four_region_layout(
        bottom_overlay_area(area, MAX_TASK_OVERLAY_ROWS),
        header_rows,
        input_rows,
    )
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
    fn overlay_layout_anchors_chat_panes_to_bottom() {
        let area = Rect::new(0, 0, 80, 24);
        let panes = split_overlay_three_pane_layout(area, 4);

        assert_eq!(panes.input.y + panes.input.height, area.height);
        assert!(
            panes.header.y > 0,
            "overlay should leave headroom on tall terminals"
        );
        assert!(panes.history.height > 0);
    }

    #[test]
    fn overlay_layout_handles_small_terminals_without_underflow() {
        let area = Rect::new(0, 0, 40, 3);
        let panes = split_overlay_three_pane_layout(area, 6);

        assert!(panes.header.y + panes.header.height <= area.height);
        assert!(panes.history.y + panes.history.height <= area.height);
        assert!(panes.input.y + panes.input.height <= area.height);
    }

    #[test]
    fn task_overlay_layout_anchors_to_bottom() {
        let area = Rect::new(0, 0, 80, 30);
        let layout = split_overlay_four_region_layout(area, 2, 3);

        assert_eq!(layout.input.y + layout.input.height, area.height);
        assert!(layout.header.y > 0);
    }
}
