use super::{InputEditor, MAX_INPUT_BYTES, file_mention_range, prev_char_boundary_in};
use crate::ui::tui::input::{KeyCode, KeyStroke, KeyModifiers};
use rstest::rstest;

#[test]
fn prev_char_boundary_handles_multibyte() {
    let text = "éa";
    assert_eq!(prev_char_boundary_in(text, 3), 2);
    assert_eq!(prev_char_boundary_in(text, 2), 0);
    assert_eq!(prev_char_boundary_in(text, 1), 0);
}

#[rstest]
#[case("", 0, None)]
#[case("   ", 1, None)]
#[case("hello world", 3, None)]
#[case("@ ", 2, None)]
#[case("@", 1, Some("@"))]
#[case("@", 0, Some("@"))]
#[case("@ ", 0, Some("@"))]
#[case("review @src/app.rs", 18, Some("@src/app.rs"))]
#[case("@one @two @three", 2, Some("@one"))]
#[case("@one @two @three", 16, Some("@three"))]
#[case("hello @ world", 6, Some("@"))]
fn file_mention_range_cases(
    #[case] input: &str,
    #[case] cursor: usize,
    #[case] expected: Option<&str>,
) {
    let result = file_mention_range(input, cursor);
    match expected {
        None => assert!(
            result.is_none(),
            "expected None for {:?} at {cursor}",
            input
        ),
        Some(s) => assert_eq!(&input[result.expect("expected Some")], s),
    }
}

#[test]
fn visual_up_down_moves_cursor_across_logical_lines() {
    let mut editor = InputEditor::new();
    editor.input_state.buffer = "alpha\nbeta\ngamma".to_string();
    editor.input_state.cursor = "alpha\nbeta\ngam".len();
    assert!(editor.move_cursor_visual_up(80));
    assert_eq!(
        &editor.input_state.buffer[..editor.input_state.cursor],
        "alpha\nbet"
    );
    assert!(editor.move_cursor_visual_down(80));
    assert_eq!(
        &editor.input_state.buffer[..editor.input_state.cursor],
        "alpha\nbeta\ngam"
    );
}

#[test]
fn visual_up_stops_at_first_row() {
    let mut editor = InputEditor::new();
    editor.input_state.buffer = "/help".to_string();
    editor.input_state.cursor = 2;
    assert!(!editor.move_cursor_visual_up(80));
    assert_eq!(editor.input_state.cursor, 2);
}

#[test]
fn ctrl_d_quits_on_empty_buffer_and_deletes_char_otherwise() {
    let mut editor = InputEditor::new();
    assert!(matches!(
        editor.apply_key(KeyStroke::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
        super::InputAction::Quit
    ));

    editor.insert_str("abcd");
    editor.input_state.cursor = 1;
    let action = editor.apply_key(KeyStroke::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert!(matches!(action, super::InputAction::None));
    assert_eq!(editor.buffer(), "acd");
}

#[test]
fn insert_str_enforces_max_input_bytes_cap() {
    let mut editor = InputEditor::new();
    editor.input_state.buffer = "x".repeat(MAX_INPUT_BYTES);
    editor.input_state.cursor = MAX_INPUT_BYTES;
    editor.insert_str("z");
    assert_eq!(editor.input_state.buffer.len(), MAX_INPUT_BYTES);
}
