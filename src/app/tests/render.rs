use super::*;
use crate::ui::render::MAX_INPUT_PANE_ROWS;

#[test]
fn test_render_not_called_when_state_unchanged() {
    let start = Instant::now();
    let mut guard = RenderGuard::with_intervals(
        Duration::from_millis(500),
        Duration::from_millis(120),
        start,
    );

    assert!(
        guard.should_draw(start, 11),
        "first render should draw because the guard starts dirty"
    );
    assert!(
        !guard.should_draw(start + Duration::from_millis(20), 11),
        "unchanged state before tick interval must not draw"
    );
    assert!(
        !guard.should_draw(start + Duration::from_millis(100), 11),
        "unchanged state still below tick interval must not draw"
    );
    assert!(
        guard.should_draw(start + Duration::from_millis(121), 11),
        "unchanged state should draw when tick interval elapses"
    );
    assert!(
        guard.should_draw(start + Duration::from_millis(122), 12),
        "changed state should mark dirty and draw immediately"
    );
}
#[test]
fn test_render_guard_poll_timeout_uses_min_tick_interval() {
    let guard = RenderGuard::with_intervals(
        Duration::from_millis(500),
        Duration::from_millis(120),
        Instant::now(),
    );
    assert_eq!(guard.poll_timeout(), Duration::from_millis(120));
}
#[test]
fn input_pane_expands_then_clamps_to_max_rows() {
    assert_eq!(input_rows_for_buffer("", 80), 1);

    let multiline = (0..12)
        .map(|idx| format!("line-{idx}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        input_rows_for_buffer(&multiline, 80),
        MAX_INPUT_PANE_ROWS as u16
    );
}
