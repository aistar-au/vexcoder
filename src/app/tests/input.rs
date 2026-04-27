use super::*;

#[test]
fn editor_cursor_navigation_and_history_cycle() {
    let mut editor = InputEditor::new();
    editor.apply_key(KeyStroke::new(KeyCode::Char('a'), KeyModifiers::NONE));
    editor.apply_key(KeyStroke::new(KeyCode::Char('b'), KeyModifiers::NONE));
    editor.apply_key(KeyStroke::new(KeyCode::Char('c'), KeyModifiers::NONE));
    editor.apply_key(KeyStroke::new(KeyCode::Left, KeyModifiers::NONE));
    editor.apply_key(KeyStroke::new(KeyCode::Left, KeyModifiers::NONE));
    editor.apply_key(KeyStroke::new(KeyCode::Char('X'), KeyModifiers::NONE));
    assert_eq!(editor.input_state.buffer, "aXbc");

    editor.input_state.buffer = "first".to_string();
    let _ = editor.submit();
    editor.input_state.buffer = "second".to_string();
    let _ = editor.submit();
    editor.apply_key(KeyStroke::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(editor.input_state.buffer, "second");
    editor.apply_key(KeyStroke::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(editor.input_state.buffer, "first");
    editor.apply_key(KeyStroke::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(editor.input_state.buffer, "second");
}

#[test]
fn editor_undo_redo_and_multiline_newline_shortcut() {
    let mut editor = InputEditor::new();
    editor.apply_key(KeyStroke::new(KeyCode::Char('a'), KeyModifiers::NONE));
    editor.apply_key(KeyStroke::new(KeyCode::Char('b'), KeyModifiers::NONE));
    editor.apply_key(KeyStroke::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(editor.input_state.buffer, "a");
    editor.apply_key(KeyStroke::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
    assert_eq!(editor.input_state.buffer, "ab");

    let mut editor2 = InputEditor::new();
    editor2.apply_key(KeyStroke::new(KeyCode::Char('a'), KeyModifiers::NONE));
    editor2.apply_key(KeyStroke::new(KeyCode::Enter, KeyModifiers::SHIFT));
    editor2.apply_key(KeyStroke::new(KeyCode::Char('b'), KeyModifiers::NONE));
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
fn large_at_path_uses_reference_block_instead_of_full_file_dump() {
    let temp = tempfile::tempdir().unwrap();
    let workflows = temp.path().join(".github").join("workflows");
    std::fs::create_dir_all(&workflows).unwrap();
    let large_body = (0..400)
        .map(|i| {
            format!("step-{i}: gh release view \"${{TAG}}\" --json assets --jq '.assets[].name'")
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(workflows.join("release.yml"), &large_body).unwrap();

    let config = config_with_workdir(temp.path());
    let mut mode = TuiMode::new_with_config(None, config);
    let mut ctx = setup_ctx();

    mode.on_user_input("@.github/workflows/release.yml".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.clone().expect("last turn input");
    assert!(turn_input.contains("[file: .github/workflows/release.yml]"));
    assert!(turn_input.contains("[inline file content omitted"));
    assert!(!turn_input.contains("step-399:"));
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
