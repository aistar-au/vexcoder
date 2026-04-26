use super::*;

#[test]
fn memory_command_renders_empty_notes_without_pulse() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(Some(notes_path));
    mode.on_user_input("/memory".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[memory] no notes"))
    );
    assert!(!mode.is_pulse_in_progress());
}

#[test]
fn memory_add_appends_to_file_and_rejects_clear_without_confirmation() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    let state_dir = temp.path().join("state");
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", state_dir.as_os_str());

    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));
    mode.on_user_input(
        "/memory add track the open build issue".to_string(),
        &mut ctx,
    );
    let content = std::fs::read_to_string(&notes_path).unwrap_or_default();
    assert!(content.contains("track the open build issue"));

    mode.on_user_input("/memory clear".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[memory] clear all notes")),
        "clear without confirmation must prompt"
    );
}

#[test]
fn build_runtime_auto_index_warms_codebase_search_index() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::state::clear_codebase_index_for_tests();
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("warm.rs"), "pub fn tui_warm_symbol() {}\n").unwrap();
    let search_config = crate::config::SearchConfig {
        enabled: true,
        auto_index: true,
        ..Default::default()
    };
    let count = crate::state::warm_codebase_index_with_config(temp.path(), &search_config);
    assert!(count.is_some() && count.unwrap() > 0);
    let names = crate::state::codebase_index_names_for_tests();
    assert!(names.iter().any(|n| n == "tui_warm_symbol"));
}
