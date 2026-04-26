use super::*;

#[test]
fn file_mention_range_tracks_token_under_cursor() {
    let input = "inspect @src/app/inp more";
    let cursor = input.find("inp").unwrap() + 3;
    let range = file_mention_range(input, cursor).expect("mention range");
    assert_eq!(&input[range], "@src/app/inp");
}

#[test]
fn file_picker_hint_marks_selected_entry_and_handles_edge_cases() {
    let hint = render_file_picker_hint(
        "inp",
        &["src/app/input.rs".into(), "src/app/inline.rs".into()],
        2,
        1,
    );
    assert!(hint.contains("> [file] src/app/inline.rs"));
    assert!(hint.contains("  [file] src/app/input.rs"));

    let empty = render_file_picker_hint("missing", &[], 0, 0);
    assert!(empty.contains("no matches for missing"));

    let clamped = render_file_picker_hint("x", &["src/x.rs".into(), "src/xy.rs".into()], 2, 999);
    assert!(clamped.contains("> [file] src/xy.rs"));
}

#[test]
fn apply_file_picker_selection_replaces_partial_token() {
    let mut editor = InputEditor::new();
    editor.insert_str("inspect @inp");
    let range = file_mention_range(editor.buffer(), editor.cursor()).expect("mention range");
    apply_file_picker_selection(&mut editor, &range, "src/app/input.rs");
    assert_eq!(editor.buffer(), "inspect @src/app/input.rs ");

    let mut editor2 = InputEditor::new();
    editor2.insert_str("look at @inp and fix");
    editor2.input_state.cursor = "look at @inp".len();
    let range2 = file_mention_range(editor2.buffer(), editor2.cursor()).expect("range");
    apply_file_picker_selection(&mut editor2, &range2, "src/app/input.rs");
    assert_eq!(editor2.buffer(), "look at @src/app/input.rs and fix");
}

#[test]
fn dismissed_file_picker_stays_suppressed_until_input_changes() {
    let input = "inspect @inp";
    let range = file_mention_range(input, input.len()).expect("range");
    let dismissed = Some((input.to_string(), range));
    assert!(file_picker_is_dismissed(
        dismissed.as_ref(),
        "inspect @inp",
        "inspect @inp".len()
    ));
    assert!(!file_picker_is_dismissed(
        dismissed.as_ref(),
        "inspect @input",
        "inspect @input".len()
    ));
    assert!(!file_picker_is_dismissed(None, "@test", 5));
}

#[test]
fn active_file_picker_finds_matching_file_in_workspace() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src/app")).unwrap();
    std::fs::write(temp.path().join("src/app/input.rs"), "fn hint() {}\n").unwrap();
    let mut config = Config::default_for_tui();
    config.working_dir = temp.path().to_path_buf();
    let mode = TuiMode::new_with_config(None, config);
    let picker = active_file_picker(&mode, "inspect @inp", "inspect @inp".len()).expect("picker");
    assert_eq!(picker.prefix, "inp");
    assert!(picker.matches.contains(&"src/app/input.rs".to_string()));
}
