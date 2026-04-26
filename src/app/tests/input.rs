use super::*;

#[test]
fn editor_cursor_navigation_and_history_cycle() {
    let mut editor = InputEditor::new();
    editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
    assert_eq!(editor.input_state.buffer, "aXbc");

    editor.input_state.buffer = "first".to_string();
    let _ = editor.submit();
    editor.input_state.buffer = "second".to_string();
    let _ = editor.submit();
    editor.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(editor.input_state.buffer, "second");
    editor.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(editor.input_state.buffer, "first");
    editor.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(editor.input_state.buffer, "second");
}

#[test]
fn editor_undo_redo_and_multiline_newline_shortcut() {
    let mut editor = InputEditor::new();
    editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(editor.input_state.buffer, "a");
    editor.apply_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
    assert_eq!(editor.input_state.buffer, "ab");

    let mut editor2 = InputEditor::new();
    editor2.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    editor2.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    editor2.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    assert_eq!(editor2.input_state.buffer, "a\nb");
}

#[test]
fn editor_submit_trims_trailing_newlines_before_history() {
    let mut editor = InputEditor::new();
    editor.input_state.buffer = "first line\nsecond line\n\n".to_string();
    editor.input_state.cursor = editor.input_state.buffer.len();
    let submitted = editor.submit().expect("submitted");
    assert_eq!(submitted, "first line\nsecond line");
    editor.history_up();
    assert_eq!(editor.input_state.buffer, "first line\nsecond line");
}

#[test]
fn at_path_injects_file_contents_into_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), "pub fn hello() {}").unwrap();

    let config = config_with_workdir(temp.path());
    let mut mode = TuiMode::new_with_config(None, config.clone());
    let mut ctx = setup_ctx();

    mode.on_user_input("@src/lib.rs".to_string(), &mut ctx);
    let lines = mode.history_lines();
    assert!(
        lines.iter().any(|l| l.contains("hello")),
        "file contents must appear in history; lines: {lines:?}"
    );
}

#[test]
fn at_path_rejects_path_outside_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let config = config_with_workdir(temp.path());
    let mut mode = TuiMode::new_with_config(None, config);
    let mut ctx = setup_ctx();
    mode.on_user_input("@/etc/passwd".to_string(), &mut ctx);
    let lines = mode.history_lines();
    assert!(
        lines.iter().any(|l| l.contains("outside")
            || l.contains("workspace")
            || l.contains("rejected")
            || l.contains("not allowed")),
        "out-of-workspace path must be rejected; lines: {lines:?}"
    );
}
