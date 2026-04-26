use super::*;

#[test]
fn fork_saves_parent_and_creates_new_task_id() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let parent_id = mode.current_task_id();
    mode.task_doc.info.active_grants.insert(
        crate::runtime::Capability::RunCommand,
        crate::runtime::ApprovalScope::Session,
    );
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();

    mode.on_user_input("/fork feature work".to_string(), &mut ctx);

    assert!(
        temp.path().join(format!("{parent_id}.json")).exists(),
        "/fork must save parent state"
    );
    assert_ne!(
        mode.current_task_id(),
        parent_id,
        "/fork must create a new task-id"
    );
    assert!(!mode.is_pulse_in_progress());

    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
}
