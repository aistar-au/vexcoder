use super::*;

#[tokio::test]
async fn overlay_blocks_input_and_clears_after_decision() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let (response_tx, _rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
        tool_name: "read_file".to_string(),
        input_preview: "{}".to_string(),
        response_tx,
    }), &mut ctx);

    mode.on_user_input("blocked".to_string(), &mut ctx);
    assert!(mode.task_doc.active_pulse.is_none(), "overlay must block input routing");

    mode.on_user_input("1".to_string(), &mut ctx);
    assert!(!mode.overlay_active(), "overlay should clear after decision");

    mode.on_user_input("resume".to_string(), &mut ctx);
    assert!(mode.task_doc.active_pulse.is_some(), "routing should resume after overlay clears");
}

#[test]
fn resume_selection_overlay_routes_numeric_input_to_task() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path().as_os_str());

    let saved = TaskState::new("task-resume-overlay".to_string());
    saved.save(temp.path()).unwrap();

    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    mode.prompt_resume_selection(vec![ResumeTaskEntry {
        dir: temp.path().to_path_buf(),
        id: "task-resume-overlay".to_string(),
        status: "Ready".to_string(),
    }]);
    mode.on_user_input("1".to_string(), &mut ctx);
    assert_eq!(mode.current_task_id(), "task-resume-overlay");
    assert!(!mode.overlay_active());
}

#[test]
fn approval_overlay_shows_correct_tool_name_and_input_preview() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let (response_tx, _rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
        tool_name: "write_file".to_string(),
        input_preview: r#"{"path":"src/new.rs"}"#.to_string(),
        response_tx,
    }), &mut ctx);

    assert!(mode.overlay_active());
    let approval = mode.overlay_state.pending_approval.as_ref().expect("pending approval");
    assert_eq!(approval.tool_name, "write_file");
    assert!(approval.input_preview.contains("src/new.rs"));
}
