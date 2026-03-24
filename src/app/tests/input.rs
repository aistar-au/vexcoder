use super::*;

#[test]
fn test_editor_cursor_navigation() {
    let mut editor = InputEditor::new();
    editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
    assert_eq!(editor.input_state.buffer, "aXbc");
}
#[test]
fn test_editor_history_up_down() {
    let mut editor = InputEditor::new();
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
fn test_editor_history_stash_restore() {
    let mut editor = InputEditor::new();

    editor.input_state.buffer = "first".to_string();
    let _ = editor.submit();
    editor.input_state.buffer = "second".to_string();
    let _ = editor.submit();

    editor.input_state.buffer = "draft".to_string();
    editor.input_state.cursor = editor.input_state.buffer.len();

    editor.history_up();
    assert_eq!(editor.input_state.buffer, "second");
    assert_eq!(editor.input_state.history_index, Some(1));

    editor.history_down();
    assert_eq!(editor.input_state.history_index, None);
    assert_eq!(editor.input_state.buffer, "draft");
    assert_eq!(editor.input_state.cursor, "draft".len());
}
#[test]
fn test_editor_submit_trims_trailing_newlines_before_history_recall() {
    let mut editor = InputEditor::new();
    editor.input_state.buffer = "first line\nsecond line\n\n".to_string();
    editor.input_state.cursor = editor.input_state.buffer.len();

    let submitted = editor.submit().expect("submitted multiline prompt");
    assert_eq!(submitted, "first line\nsecond line");

    editor.history_up();
    assert_eq!(editor.input_state.buffer, "first line\nsecond line");
    assert_eq!(editor.input_state.cursor, "first line\nsecond line".len());
}
#[test]
fn test_editor_multiline_shortcuts() {
    let mut editor = InputEditor::new();
    editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
    editor.apply_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
    assert_eq!(editor.input_state.buffer, "a\nb\nc");
}
#[test]
fn test_editor_undo_redo() {
    let mut editor = InputEditor::new();
    editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(editor.input_state.buffer, "a");
    editor.apply_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
    assert_eq!(editor.input_state.buffer, "ab");
}
#[test]
fn test_editor_paste_handling() {
    let mut editor = InputEditor::new();
    let _ = editor.apply_event(Event::Paste("hello".to_string()));
    assert_eq!(editor.input_state.buffer, "hello");
}
#[test]
fn test_input_editor_unicode_cursor_backspace_delete_safe() {
    let mut editor = InputEditor::new();
    editor.insert_str("a\u{1F600}b");
    editor.input_state.cursor = editor.input_state.buffer.len();
    editor.backspace();
    assert_eq!(editor.input_state.buffer, "a\u{1F600}");
    editor.backspace();
    assert_eq!(editor.input_state.buffer, "a");

    editor.insert_str("\u{1F600}b");
    editor.input_state.cursor = 2; // intentionally non-boundary (inside emoji codepoint)
    editor.delete();
    assert_eq!(editor.input_state.buffer, "ab");
}
#[test]
fn test_at_path_injects_file_contents() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("note.txt"), "hello from file\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input("summarize @note.txt".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("[file: note.txt]"));
    assert!(turn_input.contains("hello from file"));
    assert!(mode.is_turn_in_progress());
}
#[test]
fn test_at_path_directory_renders_listing() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("src")).unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), "pub fn hi() {}\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input("review @src".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("[dir: src]"));
    assert!(turn_input.contains("src/lib.rs"));
}
#[test]
fn test_at_path_missing_file_is_annotated() {
    let temp = tempfile::tempdir().unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input("inspect @missing.txt".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("[file: missing.txt \u{2014} not found]"));
}
#[test]
fn test_at_path_outside_workspace_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_path = outside.path().join("secret.txt");
    std::fs::write(&outside_path, "secret").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input(
        format!("inspect @{}", outside_path.to_string_lossy()),
        &mut ctx,
    );

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("[file: "));
    assert!(
        turn_input.contains("outside workspace root")
            || turn_input.contains("escapes workspace root")
            || turn_input.contains("absolute or platform-specific path not allowed"),
        "expected outside-workspace annotation, got: {turn_input}"
    );
}
#[test]
fn test_at_path_multiple_tokens_resolved_in_order() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("one.txt"), "first").unwrap();
    std::fs::write(temp.path().join("two.txt"), "second").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input("compare @one.txt with @two.txt".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    let first_idx = turn_input.find("[file: one.txt]").unwrap();
    let second_idx = turn_input.find("[file: two.txt]").unwrap();
    assert!(first_idx < second_idx);
    assert!(turn_input.contains("first"));
    assert!(turn_input.contains("second"));
}
#[test]
fn test_at_path_expanded_inside_edit_command_args() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("note.txt"), "hello from file\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input("/edit use @note.txt for context".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("[file: note.txt]"));
    assert!(turn_input.contains("hello from file"));
}
#[test]
fn test_at_path_argument_is_normalized_for_explain_command() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/explain @src/app.rs".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("explain src/app.rs"));
    assert!(!turn_input.contains("explain @src/app.rs"));
}
#[test]
fn test_at_path_expanded_inside_plan_args() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("note.txt"), "hello from file\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input("/plan use @note.txt for context".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("[file: note.txt]"));
    assert!(turn_input.contains("hello from file"));
}
#[test]
fn test_file_prompt_matches_include_repo_wide_substring_results() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src/app")).unwrap();
    std::fs::create_dir_all(temp.path().join("docs/guides")).unwrap();
    std::fs::write(temp.path().join("src/app/input.rs"), "fn a() {}\n").unwrap();
    std::fs::write(temp.path().join("src/app/input_state.rs"), "fn b() {}\n").unwrap();
    std::fs::write(temp.path().join("docs/guides/prompt-input.md"), "docs\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();

    let matches = mode.file_prompt_matches("input");
    assert!(matches.contains(&"src/app/input.rs".to_string()));
    assert!(matches.contains(&"src/app/input_state.rs".to_string()));
    assert!(matches.contains(&"docs/guides/prompt-input.md".to_string()));
}
#[test]
fn test_prompt_hint_for_slash_and_file_mentions() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src/app")).unwrap();
    std::fs::write(temp.path().join("src/app/input.rs"), "fn hint() {}\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();

    let slash_hint = mode.prompt_hint_for_input("/pl", 3);
    assert!(slash_hint.contains("mode: slash"));
    assert!(slash_hint.contains("/plan <instruction>"));

    let file_hint = mode.prompt_hint_for_input("inspect @inp", "inspect @inp".len());
    assert!(file_hint.contains("mode: file mention"));
    assert!(file_hint.contains("src/app/input.rs"));
}
#[test]
fn test_prompt_hint_for_file_mentions_uses_cursor_scoped_token() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(temp.path().join("src/bar.rs"), "fn bar() {}\n").unwrap();
    std::fs::write(temp.path().join("src/foo.rs"), "fn foo() {}\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();

    let input = "@bar refactoring @foo";
    let cursor = input.find("bar").unwrap() + 2;
    let hint = mode.prompt_hint_for_input(input, cursor);

    assert!(hint.contains("src/bar.rs"));
    assert!(!hint.contains("src/foo.rs"));
}
#[test]
fn test_bang_prefix_requires_run_command_approval() {
    let temp = tempfile::tempdir().unwrap();
    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input(successful_bang_input(), &mut ctx);

    assert!(mode.overlay_state.pending_approval.is_some());
    assert!(!mode.is_turn_in_progress());
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| { line.contains("[tool approval requested:") }));
}
