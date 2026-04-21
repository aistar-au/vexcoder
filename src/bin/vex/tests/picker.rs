use super::*;

#[test]
fn file_mention_range_tracks_token_under_cursor() {
    let input = "inspect @src/app/inp more";
    let cursor = input.find("inp").unwrap() + 3;
    let range = file_mention_range(input, cursor).expect("mention range");
    assert_eq!(&input[range], "@src/app/inp");
}

#[test]
fn file_picker_hint_marks_selected_entry() {
    let hint = render_file_picker_hint(
        "inp",
        &["src/app/input.rs".into(), "src/app/inline.rs".into()],
        2,
        1,
    );
    assert!(hint.contains("> [file] src/app/inline.rs"));
    assert!(hint.contains("  [file] src/app/input.rs"));
}

#[test]
fn apply_file_picker_selection_replaces_partial_token() {
    let mut editor = InputEditor::new();
    editor.insert_str("inspect @inp");
    let range = file_mention_range(editor.buffer(), editor.cursor()).expect("mention range");
    apply_file_picker_selection(&mut editor, &range, "src/app/input.rs");
    assert_eq!(editor.buffer(), "inspect @src/app/input.rs ");
}

#[test]
fn active_file_picker_uses_tui_file_matches() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src/app")).unwrap();
    std::fs::write(temp.path().join("src/app/input.rs"), "fn hint() {}\n").unwrap();

    let mut config = Config::default_for_tui();
    config.working_dir = temp.path().to_path_buf();
    let mode = TuiMode::new_with_config(None, config);

    let picker =
        active_file_picker(&mode, "inspect @inp", "inspect @inp".len()).expect("active picker");
    assert_eq!(picker.prefix, "inp");
    assert!(picker.matches.contains(&"src/app/input.rs".to_string()));
}

#[test]
fn active_slash_picker_surfaces_retrieval_guidance() {
    let config = Config::default_for_tui();
    let mode = TuiMode::new_with_config(None, config);

    let picker = active_slash_picker(&mode, "/to").expect("active slash picker");
    assert_eq!(picker.prefix, "/to");
    assert!(
        picker.matches.iter().any(|entry| {
            entry.command == "/tools "
                && entry.label.contains("retrieve + context")
                && entry
                    .label
                    .contains("tool directory plus retrieval workflow guidance")
        }),
        "picker matches: {:?}",
        picker.matches
    );
}

#[test]
fn dismissed_file_picker_stays_suppressed_until_input_changes() {
    let input = "inspect @inp";
    let range = file_mention_range(input, input.len()).expect("mention range");
    let dismissed = Some((input.to_string(), range));

    assert!(file_picker_is_dismissed(
        dismissed.as_ref(),
        "inspect @inp",
        "inspect @inp".len()
    ));
    assert!(file_picker_is_dismissed(
        dismissed.as_ref(),
        "inspect @inp",
        "inspect @i".len()
    ));
    assert!(!file_picker_is_dismissed(
        dismissed.as_ref(),
        "inspect @input",
        "inspect @input".len()
    ));
}

#[test]
fn render_file_picker_hint_empty_matches_no_prefix() {
    let hint = render_file_picker_hint("", &[], 0, 0);
    assert!(hint.contains("[file] no files available"), "hint: {hint}");
}

#[test]
fn render_file_picker_hint_empty_matches_with_prefix() {
    let hint = render_file_picker_hint("nonexist", &[], 0, 0);
    assert!(
        hint.contains("[file] no matches for nonexist"),
        "hint: {hint}"
    );
}

#[test]
fn render_file_picker_hint_clamps_selected_past_end() {
    let hint = render_file_picker_hint("x", &["src/x.rs".into(), "src/xy.rs".into()], 2, 999);
    assert!(
        hint.contains("> [file] src/xy.rs"),
        "should clamp to last entry: {hint}"
    );
}

#[test]
fn render_file_picker_hint_single_match() {
    let hint = render_file_picker_hint("exact", &["src/exact.rs".into()], 1, 0);
    assert!(hint.contains("[file] 1 match(es)"));
    assert!(hint.contains("> [file] src/exact.rs"));
}

#[test]
fn apply_file_picker_selection_bare_at_replaces_correctly() {
    let mut editor = InputEditor::new();
    editor.insert_str("@");
    let range = file_mention_range(editor.buffer(), editor.cursor()).expect("range");
    apply_file_picker_selection(&mut editor, &range, "src/main.rs");
    assert_eq!(editor.buffer(), "@src/main.rs ");
}

#[test]
fn apply_file_picker_selection_mid_sentence() {
    let mut editor = InputEditor::new();
    editor.insert_str("look at @inp and fix");
    editor.input_state.cursor = "look at @inp".len();
    let range = file_mention_range(editor.buffer(), editor.cursor()).expect("range");
    apply_file_picker_selection(&mut editor, &range, "src/app/input.rs");
    assert_eq!(editor.buffer(), "look at @src/app/input.rs and fix");
}

#[test]
fn apply_file_picker_selection_already_has_trailing_space() {
    let mut editor = InputEditor::new();
    editor.insert_str("@src ");
    editor.input_state.cursor = 4;
    let range = file_mention_range(editor.buffer(), editor.cursor()).expect("range");
    apply_file_picker_selection(&mut editor, &range, "src/lib.rs");
    assert_eq!(editor.buffer(), "@src/lib.rs ");
}

#[test]
fn file_picker_is_dismissed_none_returns_false() {
    assert!(!file_picker_is_dismissed(None, "@test", 5));
}

#[test]
fn active_file_picker_no_at_returns_none() {
    let config = Config::default_for_tui();
    let mode = TuiMode::new_with_config(None, config);

    assert!(active_file_picker(&mode, "hello world", 5).is_none());
}

#[test]
fn active_file_picker_bare_at_returns_all_matches() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(temp.path().join("src/a.rs"), "").unwrap();
    std::fs::write(temp.path().join("src/b.rs"), "").unwrap();

    let mut config = Config::default_for_tui();
    config.working_dir = temp.path().to_path_buf();
    let mode = TuiMode::new_with_config(None, config);

    let picker = active_file_picker(&mode, "@", 1).expect("bare @ picker");
    assert_eq!(picker.prefix, "");
    assert!(picker.matches.len() >= 2, "matches: {:?}", picker.matches);
}

#[test]
fn dismissed_file_picker_clears_on_new_at_token() {
    let input = "inspect @inp";
    let range = file_mention_range(input, input.len()).expect("range");
    let dismissed = Some((input.to_string(), range));

    assert!(!file_picker_is_dismissed(
        dismissed.as_ref(),
        "review @other",
        "review @other".len()
    ));
}

#[test]
fn active_file_picker_includes_directory_entries() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src/ui")).unwrap();
    std::fs::write(temp.path().join("src/ui/editor.rs"), "").unwrap();

    let mut config = Config::default_for_tui();
    config.working_dir = temp.path().to_path_buf();
    let mode = TuiMode::new_with_config(None, config);

    let picker = active_file_picker(&mode, "@", 1).expect("bare @ picker");
    assert!(
        picker.matches.iter().any(|m| m == "src/"),
        "should include src/ dir: {:?}",
        picker.matches
    );
    assert!(
        picker.matches.iter().any(|m| m == "src/ui/"),
        "should include src/ui/ dir: {:?}",
        picker.matches
    );
}

#[test]
fn file_picker_directory_entry_matches_prefix() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src/ui")).unwrap();
    std::fs::write(temp.path().join("src/ui/editor.rs"), "").unwrap();

    let mut config = Config::default_for_tui();
    config.working_dir = temp.path().to_path_buf();
    let mode = TuiMode::new_with_config(None, config);

    let picker = active_file_picker(&mode, "@src", 4).expect("prefix picker");
    assert!(
        picker.matches.iter().any(|m| m == "src/"),
        "should match src/ directory: {:?}",
        picker.matches
    );
    assert!(
        picker.matches.iter().any(|m| m == "src/ui/"),
        "should match src/ui/ directory: {:?}",
        picker.matches
    );
}

#[test]
fn apply_file_picker_selection_directory_keeps_picker_open() {
    let mut editor = InputEditor::new();
    editor.insert_str("@src");
    let range = file_mention_range(editor.buffer(), editor.cursor()).expect("range");
    apply_file_picker_selection(&mut editor, &range, "src/");
    assert_eq!(editor.buffer(), "@src/");
}

#[test]
fn apply_file_picker_selection_file_adds_space() {
    let mut editor = InputEditor::new();
    editor.insert_str("@src/ui/ed");
    let range = file_mention_range(editor.buffer(), editor.cursor()).expect("range");
    apply_file_picker_selection(&mut editor, &range, "src/ui/editor.rs");
    assert_eq!(editor.buffer(), "@src/ui/editor.rs ");
}

#[test]
fn file_picker_directory_drill_shows_children() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src/ui")).unwrap();
    std::fs::write(temp.path().join("src/ui/editor.rs"), "").unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), "").unwrap();

    let mut config = Config::default_for_tui();
    config.working_dir = temp.path().to_path_buf();
    let mode = TuiMode::new_with_config(None, config);

    let picker = active_file_picker(&mode, "@src/", "@src/".len()).expect("picker");
    assert_eq!(picker.prefix, "src/");
    assert_eq!(picker.total_matches, 2);
    assert!(
        picker.matches.iter().any(|m| m == "src/ui/"),
        "should include dir: {:?}",
        picker.matches
    );
    assert!(
        picker.matches.iter().any(|m| m == "src/lib.rs"),
        "should include file: {:?}",
        picker.matches
    );
    assert!(
        !picker.matches.iter().any(|m| m == "src/ui/editor.rs"),
        "should NOT include nested file: {:?}",
        picker.matches
    );
}

#[test]
fn file_picker_directory_filter_reports_total_children() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src/ui")).unwrap();
    std::fs::write(temp.path().join("src/ui/editor.rs"), "").unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), "").unwrap();

    let mut config = Config::default_for_tui();
    config.working_dir = temp.path().to_path_buf();
    let mode = TuiMode::new_with_config(None, config);

    let picker = active_file_picker(&mode, "@src/u", "@src/u".len()).expect("picker");
    assert_eq!(picker.matches, vec!["src/ui/".to_string()]);
    assert_eq!(picker.total_matches, 2);

    let overlay = build_file_overlay(&picker.prefix, &picker.matches, picker.total_matches, 0);
    assert!(
        overlay[0].text.contains("1 shown of 2 in src/"),
        "header should show filtered and total directory counts: {:?}",
        overlay[0]
    );

    let hint = render_file_picker_hint(&picker.prefix, &picker.matches, picker.total_matches, 0);
    assert!(
        hint.contains("[file] 1 shown of 2 in src/"),
        "hint should show filtered and total directory counts: {hint}"
    );
}

#[test]
fn slash_prefix_token_bare_slash() {
    assert_eq!(slash_prefix_token("/"), Some("/"));
}

#[test]
fn slash_prefix_token_with_command() {
    assert_eq!(slash_prefix_token("/edit something"), Some("/edit"));
}

#[test]
fn slash_prefix_token_leading_whitespace() {
    assert_eq!(slash_prefix_token("  /ed"), Some("/ed"));
}

#[test]
fn slash_prefix_token_no_slash() {
    assert!(slash_prefix_token("hello world").is_none());
}

#[test]
fn slash_prefix_token_empty() {
    assert!(slash_prefix_token("").is_none());
}

#[test]
fn active_slash_picker_bare_slash_returns_all() {
    let config = Config::default_for_tui();
    let mode = TuiMode::new_with_config(None, config);

    let picker = active_slash_picker(&mode, "/").expect("bare / picker");
    assert_eq!(picker.prefix, "/");
    assert!(
        picker.matches.len() > 5,
        "should return many commands: {:?}",
        picker.matches.len()
    );
}

#[test]
fn active_slash_picker_partial_filters() {
    let config = Config::default_for_tui();
    let mode = TuiMode::new_with_config(None, config);

    let picker = active_slash_picker(&mode, "/ed").expect("partial picker");
    assert!(
        picker
            .matches
            .iter()
            .any(|m| m.command.starts_with("/edit")),
        "should contain /edit: {:?}",
        picker.matches
    );
    assert!(
        !picker
            .matches
            .iter()
            .any(|m| m.command.starts_with("/quit")),
        "should not contain /quit"
    );
}

#[test]
fn active_slash_picker_no_match_returns_none() {
    let config = Config::default_for_tui();
    let mode = TuiMode::new_with_config(None, config);

    assert!(active_slash_picker(&mode, "/zzzznotexist").is_none());
}

#[test]
fn active_slash_picker_non_slash_returns_none() {
    let config = Config::default_for_tui();
    let mode = TuiMode::new_with_config(None, config);

    assert!(active_slash_picker(&mode, "hello").is_none());
}

#[test]
fn render_slash_picker_hint_shows_commands() {
    use vexcoder::app::SlashPickerMatch;

    let matches = vec![
        SlashPickerMatch {
            command: "/edit ".into(),
            label: "/edit <instruction> · edit + inspect · edit loop that may patch files".into(),
        },
        SlashPickerMatch {
            command: "/explain ".into(),
            label:
                "/explain [path] · retrieve + context · read-only explanation with context assembly"
                    .into(),
        },
    ];
    let hint = render_slash_picker_hint(&matches, 0);
    assert!(hint.contains("mode: slash"), "hint: {hint}");
    assert!(hint.contains("> /edit"), "selected marker: {hint}");
    assert!(hint.contains("  /explain"), "unselected: {hint}");
}

#[test]
fn render_slash_picker_hint_empty() {
    let hint = render_slash_picker_hint(&[], 0);
    assert!(hint.contains("mode: slash"), "hint: {hint}");
    assert!(!hint.contains(">"), "no selection when empty: {hint}");
}

#[test]
fn render_slash_picker_hint_clamps_selected() {
    use vexcoder::app::SlashPickerMatch;

    let matches = vec![SlashPickerMatch {
        command: "/edit ".into(),
        label: "/edit".into(),
    }];
    let hint = render_slash_picker_hint(&matches, 999);
    assert!(hint.contains("> /edit"), "should clamp: {hint}");
}

#[test]
fn apply_slash_picker_selection_replaces_input() {
    let mut editor = InputEditor::new();
    editor.insert_str("/ed");
    apply_slash_picker_selection(&mut editor, "/edit ");
    assert_eq!(editor.buffer(), "/edit ");
}

#[test]
fn apply_slash_picker_selection_from_bare_slash() {
    let mut editor = InputEditor::new();
    editor.insert_str("/");
    apply_slash_picker_selection(&mut editor, "/explain ");
    assert_eq!(editor.buffer(), "/explain ");
}

#[test]
fn build_file_overlay_bare_at_returns_entries() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(temp.path().join("src/a.rs"), "").unwrap();
    std::fs::write(temp.path().join("src/b.rs"), "").unwrap();

    let mut config = Config::default_for_tui();
    config.working_dir = temp.path().to_path_buf();
    let mode = TuiMode::new_with_config(None, config);

    let picker = active_file_picker(&mode, "@", 1).expect("bare @ picker");
    let overlay = build_file_overlay(&picker.prefix, &picker.matches, picker.total_matches, 0);

    assert!(
        overlay.len() >= 2,
        "overlay must have header + entries, got: {overlay:?}"
    );
    assert!(
        overlay[0].text.contains("match(es)"),
        "header: {}",
        overlay[0].text
    );
    let selected_count = overlay.iter().filter(|l| l.selected).count();
    assert_eq!(selected_count, 1, "exactly one selected line");
    let selected = overlay.iter().find(|l| l.selected).unwrap();
    assert!(
        selected.text.starts_with('>'),
        "selected must have > marker: {}",
        selected.text
    );
}

#[test]
fn build_file_overlay_empty_matches_shows_hint() {
    let overlay = build_file_overlay("nonexist", &[], 0, 0);
    assert_eq!(overlay.len(), 1);
    assert!(
        overlay[0].text.contains("no matches for nonexist"),
        "{}",
        overlay[0].text
    );
}

#[test]
fn build_file_overlay_bare_at_empty_shows_type_hint() {
    let overlay = build_file_overlay("", &[], 0, 0);
    assert_eq!(overlay.len(), 1);
    assert!(
        overlay[0].text.contains("type to search"),
        "{}",
        overlay[0].text
    );
}

#[test]
fn build_file_overlay_navigates_selection() {
    let matches: Vec<String> = vec!["src/a.rs".into(), "src/b.rs".into(), "src/c.rs".into()];

    let overlay_0 = build_file_overlay("", &matches, matches.len(), 0);
    let overlay_1 = build_file_overlay("", &matches, matches.len(), 1);
    let overlay_2 = build_file_overlay("", &matches, matches.len(), 2);

    let sel_0 = overlay_0.iter().find(|l| l.selected).unwrap();
    assert!(sel_0.text.contains("src/a.rs"), "sel 0: {}", sel_0.text);
    let sel_1 = overlay_1.iter().find(|l| l.selected).unwrap();
    assert!(sel_1.text.contains("src/b.rs"), "sel 1: {}", sel_1.text);
    let sel_2 = overlay_2.iter().find(|l| l.selected).unwrap();
    assert!(sel_2.text.contains("src/c.rs"), "sel 2: {}", sel_2.text);
}

#[test]
fn build_slash_overlay_returns_entries_with_selection() {
    use vexcoder::app::SlashPickerMatch;

    let matches = vec![
        SlashPickerMatch {
            command: "/edit ".into(),
            label: "/edit <instruction>".into(),
        },
        SlashPickerMatch {
            command: "/explain ".into(),
            label: "/explain [path]".into(),
        },
    ];
    let overlay = build_slash_overlay(&matches, 0);

    assert!(overlay.len() >= 3, "header + 2 entries: {overlay:?}");
    assert!(
        overlay[0].text.contains("command(s)"),
        "header: {}",
        overlay[0].text
    );
    let selected = overlay.iter().find(|l| l.selected).unwrap();
    assert!(
        selected.text.contains("/edit"),
        "selected: {}",
        selected.text
    );
}

#[test]
fn build_slash_overlay_empty_returns_empty() {
    let overlay = build_slash_overlay(&[], 0);
    assert!(overlay.is_empty());
}

#[test]
fn file_overlay_clamps_selected_past_end() {
    let matches: Vec<String> = vec!["src/a.rs".into(), "src/b.rs".into()];
    let overlay = build_file_overlay("", &matches, matches.len(), 999);
    let selected = overlay.iter().find(|l| l.selected).unwrap();
    assert!(
        selected.text.contains("src/b.rs"),
        "should clamp to last: {}",
        selected.text
    );
}

#[test]
fn file_overlay_integration_with_active_picker() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src/ui")).unwrap();
    std::fs::write(temp.path().join("src/ui/editor.rs"), "fn main() {}").unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), "").unwrap();

    let mut config = Config::default_for_tui();
    config.working_dir = temp.path().to_path_buf();
    let mode = TuiMode::new_with_config(None, config);

    let picker = active_file_picker(&mode, "@", 1).expect("@ must activate picker");
    assert!(!picker.matches.is_empty(), "@ must return file matches");

    let overlay = build_file_overlay(&picker.prefix, &picker.matches, picker.total_matches, 0);
    assert!(overlay.len() >= 2, "overlay must show menu entries for @");

    let picker2 = active_file_picker(&mode, "@src/", 5).expect("@src/ picker");
    let overlay2 = build_file_overlay(&picker2.prefix, &picker2.matches, picker2.total_matches, 0);
    assert!(
        overlay2.iter().any(|l| l.text.contains("src/ui/")),
        "directory drill-down must show children: {overlay2:?}"
    );
}
