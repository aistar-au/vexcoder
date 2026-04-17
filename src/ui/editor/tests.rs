use super::{InputEditor, MAX_INPUT_BYTES, file_mention_range, prev_char_boundary_in};
use crate::ui::input_metrics::{cursor_row_col, visual_layout, visual_row_bounds};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn shared_prev_char_boundary_handles_multibyte_boundaries() {
    let text = "éa";
    assert_eq!(prev_char_boundary_in(text, 3), 2);
    assert_eq!(prev_char_boundary_in(text, 2), 0);
    assert_eq!(prev_char_boundary_in(text, 1), 0);
}

#[test]
fn file_mention_range_tracks_cursor_scoped_token() {
    let input = "@bar refactoring @foo";
    let cursor = input.find("bar").unwrap() + 2;
    let range = file_mention_range(input, cursor).expect("mention range");
    assert_eq!(&input[range], "@bar");
}

#[test]
fn replace_range_uses_clamped_start_for_reversed_ranges() {
    let mut editor = InputEditor::new();
    editor.insert_str("éa");
    editor.replace_range(3, 1, "x");
    assert_eq!(editor.buffer(), "xéa");
    assert_eq!(editor.cursor(), 1);
}

#[test]
fn visual_up_down_moves_within_multiline_prompt() {
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
fn visual_up_down_uses_wrapped_rows_for_long_prompt_lines() {
    let mut editor = InputEditor::new();
    editor.input_state.buffer = "/plan @src/ui/render/mod.rs with more context".to_string();
    editor.input_state.cursor = editor.input_state.buffer.len();

    let (start_row, start_col) = cursor_row_col(&editor.input_state.buffer, editor.cursor(), 12);
    assert!(
        start_row > 0,
        "fixture must wrap to more than one visual row"
    );

    assert!(editor.move_cursor_visual_up(12));
    let (up_row, up_col) = cursor_row_col(&editor.input_state.buffer, editor.cursor(), 12);
    assert_eq!(up_row + 1, start_row);
    assert_eq!(up_col, start_col);

    assert!(editor.move_cursor_visual_down(12));
    let (down_row, down_col) = cursor_row_col(&editor.input_state.buffer, editor.cursor(), 12);
    assert_eq!(down_row, start_row);
    assert_eq!(down_col, start_col);
}

#[test]
fn visual_up_stops_at_first_prompt_row() {
    let mut editor = InputEditor::new();
    editor.input_state.buffer = "/help".to_string();
    editor.input_state.cursor = 2;

    assert!(!editor.move_cursor_visual_up(80));
    assert_eq!(editor.input_state.cursor, 2);
}

#[test]
fn visual_home_end_stay_within_multiline_prompt_row() {
    let mut editor = InputEditor::new();
    editor.input_state.buffer = "alpha\nbeta\ngamma".to_string();
    editor.input_state.cursor = "alpha\nbet".len();

    assert!(editor.move_cursor_visual_home(80));
    assert_eq!(editor.input_state.cursor, "alpha\n".len());

    assert!(editor.move_cursor_visual_end(80));
    assert_eq!(editor.input_state.cursor, "alpha\nbeta".len());
}

#[test]
fn visual_home_end_use_wrapped_rows_for_long_prompt_lines() {
    let mut editor = InputEditor::new();
    editor.input_state.buffer = "/plan @src/ui/render/mod.rs with more context".to_string();
    editor.input_state.cursor = editor.input_state.buffer.len();

    let (row, _) = cursor_row_col(&editor.input_state.buffer, editor.cursor(), 12);
    let (row_start, row_end) = visual_row_bounds(&editor.input_state.buffer, row, 12);

    assert!(editor.move_cursor_visual_home(12));
    assert_eq!(editor.input_state.cursor, row_start);

    assert!(editor.move_cursor_visual_end(12));
    assert_eq!(editor.input_state.cursor, row_end);
}

#[test]
fn visual_layout_tracks_exact_wrap_boundary_as_empty_following_row() {
    let layout = visual_layout("abcd", 4, 4);

    assert_eq!(layout.cursor_row(), 1);
    assert_eq!(layout.cursor_col(), 0);
    assert_eq!(layout.row_count(), 2);
    assert_eq!(layout.row_bounds(1), (4, 4));
}

#[test]
fn visual_home_end_do_not_jump_to_buffer_start_at_exact_wrap_boundary() {
    let mut editor = InputEditor::new();
    editor.input_state.buffer = "abcd".to_string();
    editor.input_state.cursor = 4;

    assert!(!editor.move_cursor_visual_home(4));
    assert_eq!(editor.input_state.cursor, 4);

    assert!(editor.move_cursor_visual_up(4));
    assert_eq!(editor.input_state.cursor, 0);

    assert!(editor.move_cursor_visual_down(4));
    assert_eq!(editor.input_state.cursor, 4);
}

#[test]
fn visual_layout_treats_wrapped_row_start_as_current_row() {
    let layout = visual_layout("abcdz", 4, 4);

    assert_eq!(layout.cursor_row(), 1);
    assert_eq!(layout.cursor_col(), 0);
    assert_eq!(layout.row_bounds(1), (4, 5));
}

// -- @ mention edge cases -------------------------------------------------

#[test]
fn file_mention_range_bare_at_returns_token() {
    let range = file_mention_range("@", 1).expect("bare @ is a mention");
    assert_eq!(range, 0..1);
}

#[test]
fn file_mention_range_bare_at_cursor_start() {
    let range = file_mention_range("@", 0).expect("bare @ cursor at start");
    assert_eq!(range, 0..1);
}

#[test]
fn file_mention_range_empty_buffer_returns_none() {
    assert!(file_mention_range("", 0).is_none());
}

#[test]
fn file_mention_range_whitespace_only_returns_none() {
    assert!(file_mention_range("   ", 1).is_none());
}

#[test]
fn file_mention_range_no_at_prefix_returns_none() {
    assert!(file_mention_range("hello world", 3).is_none());
}

#[test]
fn file_mention_range_at_followed_by_space_cursor_past_space() {
    // Buffer: "@ ", cursor at 2 (after the space) — no active mention.
    assert!(file_mention_range("@ ", 2).is_none());
}

#[test]
fn file_mention_range_at_followed_by_space_cursor_on_at() {
    // Buffer: "@ ", cursor at 0 — still on the @ token.
    let range = file_mention_range("@ ", 0).expect("cursor on @");
    assert_eq!(range, 0..1);
}

#[test]
fn file_mention_range_at_with_path_at_end_of_buffer() {
    let input = "review @src/app.rs";
    let range = file_mention_range(input, input.len()).expect("at end");
    assert_eq!(&input[range], "@src/app.rs");
}

#[test]
fn file_mention_range_at_with_path_cursor_in_middle() {
    let input = "review @src/app.rs more";
    let cursor = input.find("app").unwrap();
    let range = file_mention_range(input, cursor).expect("cursor mid-token");
    assert_eq!(&input[range], "@src/app.rs");
}

#[test]
fn file_mention_range_multiple_at_tokens_first() {
    let input = "@one @two @three";
    let cursor = 2; // inside "one"
    let range = file_mention_range(input, cursor).expect("first token");
    assert_eq!(&input[range], "@one");
}

#[test]
fn file_mention_range_multiple_at_tokens_last() {
    let input = "@one @two @three";
    let cursor = input.len();
    let range = file_mention_range(input, cursor).expect("last token");
    assert_eq!(&input[range], "@three");
}

#[test]
fn file_mention_range_unicode_path() {
    let input = "@données/résumé.txt";
    let range = file_mention_range(input, input.len()).expect("unicode path");
    assert_eq!(&input[range], input);
}

#[test]
fn file_mention_range_adjacent_to_newline() {
    let input = "line1\n@file.rs\nline3";
    let cursor = input.find("file").unwrap() + 2;
    let range = file_mention_range(input, cursor).expect("after newline");
    assert_eq!(&input[range], "@file.rs");
}

#[test]
fn file_mention_range_bare_at_between_words() {
    let input = "hello @ world";
    let cursor = 6; // on the @
    let range = file_mention_range(input, cursor).expect("bare @ between words");
    assert_eq!(&input[range], "@");
}

// -- ADR-021 Item 18: buffer cap ------------------------------------------

#[test]
fn insert_str_silently_drops_input_above_cap() {
    let mut editor = InputEditor::new();
    // Fill to exactly the cap.
    let filler = "x".repeat(MAX_INPUT_BYTES);
    editor.input_state.buffer = filler.clone();
    editor.input_state.cursor = MAX_INPUT_BYTES;
    // Attempt to insert one more byte — must be silently ignored.
    editor.insert_str("z");
    assert_eq!(editor.input_state.buffer.len(), MAX_INPUT_BYTES);
    assert_eq!(editor.input_state.buffer, filler);
}

#[test]
fn insert_str_allows_input_up_to_cap() {
    let mut editor = InputEditor::new();
    let filler = "x".repeat(MAX_INPUT_BYTES - 1);
    editor.input_state.buffer = filler;
    editor.input_state.cursor = MAX_INPUT_BYTES - 1;
    // One byte below the cap — must succeed.
    editor.insert_str("z");
    assert_eq!(editor.input_state.buffer.len(), MAX_INPUT_BYTES);
}

#[test]
fn ctrl_d_quits_only_when_buffer_is_empty() {
    let mut editor = InputEditor::new();
    let action = editor.apply_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert!(matches!(action, super::InputAction::Quit));
}

#[test]
fn ctrl_d_deletes_character_when_buffer_is_not_empty() {
    let mut editor = InputEditor::new();
    editor.insert_str("abcd");
    editor.input_state.cursor = 1;

    let action = editor.apply_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));

    assert!(matches!(action, super::InputAction::None));
    assert_eq!(editor.buffer(), "acd");
    assert_eq!(editor.cursor(), 1);
}
