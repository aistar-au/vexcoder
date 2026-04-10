use super::*;

// -- /fork ----------------------------------------------------------------

#[test]
fn test_tui_fork_saves_parent_before_branching() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let parent_id = mode.current_task_id();
    let mut ctx = setup_ctx();
    mode.on_user_input("/fork".to_string(), &mut ctx);

    let parent_file = temp.path().join(format!("{parent_id}.json"));
    assert!(parent_file.exists(), "/fork must save parent state file");
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_fork_creates_new_task_id() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let parent_id = mode.current_task_id();
    mode.task_doc.meta.active_grants.insert(
        crate::runtime::Capability::RunCommand,
        crate::runtime::ApprovalScope::Session,
    );
    // In the new model changed_files live in completed turns, not on current_task.
    // Pre-populate a completed turn to carry the file across the fork.
    mode.task_doc.meta.status = crate::runtime::TaskStatus::Running;
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();

    mode.on_user_input("/fork feature work".to_string(), &mut ctx);

    assert_ne!(
        mode.current_task_id(),
        parent_id,
        "/fork must assign a new task-id"
    );
    assert!(mode.current_task_id().ends_with("-feature-work"));
    assert!(mode
        .task_doc
        .meta
        .active_grants
        .contains_key(&crate::runtime::Capability::RunCommand));
    // NOTE: In the document-projector model, forks start with empty completed_turns;
    // changed_files are per-turn and are not inherited by the forked task.
    assert_eq!(
        mode.task_doc.meta.status,
        crate::runtime::TaskStatus::Running
    );
    assert_eq!(mode.history_lines().len(), 1, "/fork must reset transcript");
    assert!(
        mode.history_lines()[0].contains(&format!("branched from {parent_id}")),
        "expected fork confirmation in history"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_fork_does_not_copy_conversation() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();
    mode.on_user_input("/fork".to_string(), &mut ctx);

    assert!(
        !mode.history_lines().iter().any(|l| l == "stale transcript"),
        "/fork must clear prior transcript state"
    );
    assert!(
        !mode.is_turn_in_progress(),
        "/fork must not start a model turn"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_fork_aborts_on_save_failure() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let blocking_path = temp.path().join("state-file");
    std::fs::write(&blocking_path, "occupied").unwrap();
    std::env::set_var("VEX_STATE_DIR", blocking_path.as_os_str());

    let mut mode = TuiMode::new();
    let original_id = mode.current_task_id();
    let mut ctx = setup_ctx();
    mode.on_user_input("/fork".to_string(), &mut ctx);

    assert_eq!(
        mode.current_task_id(),
        original_id,
        "/fork must not change task-id when parent save fails"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[fork] save failed")),
        "expected save failure message"
    );
    std::env::remove_var("VEX_STATE_DIR");
}
