use super::*;

// -- /compact -------------------------------------------------------------

#[test]
fn test_tui_compact_resets_conversation_history() {
    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();

    mode.on_user_input("/compact".to_string(), &mut ctx);

    // 1 confirmation line (pre_session_notice) + 1 compaction boundary row
    assert_eq!(
        mode.history_lines().len(),
        2,
        "/compact must reset the transcript with boundary marker"
    );
    assert!(
        mode.history_lines()[0].starts_with("[compacted: "),
        "expected compacted confirmation, got: {}",
        mode.history_lines()[0]
    );
    assert!(
        mode.history_lines()[1].starts_with("[context compacted: "),
        "expected compaction boundary marker, got: {}",
        mode.history_lines()[1]
    );
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_tui_compact_persists_cleared_turns() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    mode.task_doc
        .completed_turns
        .push(crate::runtime::task_document::TurnDocument {
            turn_index: 0,
            input: "draft a plan".to_string(),
            entries: vec![],
            outcome: crate::runtime::task_document::TurnOutcome::Completed,
            changed_files: vec![],
            command_history: vec![],
            tokens: crate::usage::TurnTokens {
                ..Default::default()
            },
            started_at_ms: 0,
            completed_at_ms: 1,
            ttft_ms: None,
            timings: None,
        });
    mode.persist_task_document();

    let task_id = mode.current_task_id();
    let mut ctx = setup_ctx();
    mode.on_user_input("/compact".to_string(), &mut ctx);

    let saved = TaskState::load(temp.path(), &task_id).unwrap();
    assert!(
        saved.turns.is_empty(),
        "/compact must persist cleared turns"
    );

    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
}

#[test]
fn test_tui_compact_preserves_task_id_and_grants() {
    let mut mode = TuiMode::new();
    let original_id = mode.current_task_id();
    mode.task_doc.info.active_grants.insert(
        crate::runtime::Capability::RunCommand,
        crate::runtime::ApprovalScope::Session,
    );
    let mut ctx = setup_ctx();

    mode.on_user_input("/compact".to_string(), &mut ctx);

    assert_eq!(
        mode.current_task_id(),
        original_id,
        "/compact must not change task-id"
    );
    assert!(
        mode.task_doc
            .info
            .active_grants
            .contains_key(&crate::runtime::Capability::RunCommand),
        "/compact must preserve active grants"
    );
    assert!(
        !mode.is_turn_in_progress(),
        "/compact must clear active edit-loop state"
    );
}

#[test]
fn test_tui_compact_clears_active_edit_loop() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/compact".to_string(), &mut ctx);

    assert!(
        !mode.is_turn_in_progress(),
        "/compact must clear active edit-loop state"
    );
}
