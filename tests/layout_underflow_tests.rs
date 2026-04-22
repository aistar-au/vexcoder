use vexapi::ui::layout::{Rect, split_four_region_layout, split_three_pane_layout};

fn assert_rect_within(area: Rect, rect: Rect) {
    assert!(
        rect.x.saturating_add(rect.width) <= area.x.saturating_add(area.width),
        "rect exceeds horizontal bounds: area={area:?} rect={rect:?}"
    );
    assert!(
        rect.y.saturating_add(rect.height) <= area.y.saturating_add(area.height),
        "rect exceeds vertical bounds: area={area:?} rect={rect:?}"
    );
}

#[test]
fn test_three_pane_layout_handles_small_terminal_sizes() {
    for area in [
        Rect::new(0, 0, 10, 3),
        Rect::new(0, 0, 20, 5),
        Rect::new(0, 0, 40, 12),
    ] {
        let panes = split_three_pane_layout(area, 4);
        assert_rect_within(area, panes.header);
        assert_rect_within(area, panes.history);
        assert_rect_within(area, panes.input);
        assert!(panes.header.y <= panes.history.y);
        assert!(panes.history.y <= panes.input.y);
    }
}

#[test]
fn test_four_region_layout_handles_small_terminal_sizes() {
    for area in [
        Rect::new(0, 0, 10, 3),
        Rect::new(0, 0, 20, 5),
        Rect::new(0, 0, 40, 12),
    ] {
        let layout = split_four_region_layout(area, 1, 3);
        assert_rect_within(area, layout.header);
        assert_rect_within(area, layout.activity);
        assert_rect_within(area, layout.output);
        assert_rect_within(area, layout.input);
        assert!(layout.header.y <= layout.activity.y);
        assert!(layout.activity.y <= layout.output.y);
        assert!(layout.output.y <= layout.input.y);
    }
}

#[test]
fn test_four_region_layout_keeps_prompt_fixed_on_tall_terminals() {
    let area = Rect::new(0, 0, 80, 40);
    let layout = split_four_region_layout(area, 0, 3);

    assert_rect_within(area, layout.header);
    assert_rect_within(area, layout.activity);
    assert_rect_within(area, layout.output);
    assert_rect_within(area, layout.input);
    assert_eq!(layout.activity.height, 0);
    assert_eq!(layout.input.height, 3);
    assert!(
        layout.output.height >= 36,
        "output pane should remain the dominant region on tall displays"
    );
}
