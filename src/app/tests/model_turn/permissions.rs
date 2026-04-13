use super::*;

#[test]
fn test_edit_command_grants_task_permissions() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/edit refactor src/app.rs".to_string(), &mut ctx);

    assert_eq!(
        mode.task_doc.info.active_grants.get(&Capability::WriteFile),
        Some(&ApprovalScope::Task)
    );
    assert_eq!(
        mode.task_doc
            .info
            .active_grants
            .get(&Capability::ApplyPatch),
        Some(&ApprovalScope::Task)
    );
    assert_eq!(
        mode.task_doc
            .info
            .active_grants
            .get(&Capability::RunCommand),
        Some(&ApprovalScope::Task)
    );
}

#[test]
fn test_edit_command_preserves_session_permissions() {
    let mut mode = TuiMode::new();
    mode.task_doc
        .info
        .active_grants
        .insert(Capability::WriteFile, ApprovalScope::Session);
    mode.task_doc
        .info
        .active_grants
        .insert(Capability::ApplyPatch, ApprovalScope::Session);
    mode.task_doc
        .info
        .active_grants
        .insert(Capability::RunCommand, ApprovalScope::Session);
    let mut ctx = setup_ctx();

    mode.on_user_input("/edit refactor src/app.rs".to_string(), &mut ctx);

    assert_eq!(
        mode.task_doc.info.active_grants.get(&Capability::WriteFile),
        Some(&ApprovalScope::Session)
    );
    assert_eq!(
        mode.task_doc
            .info
            .active_grants
            .get(&Capability::ApplyPatch),
        Some(&ApprovalScope::Session)
    );
    assert_eq!(
        mode.task_doc
            .info
            .active_grants
            .get(&Capability::RunCommand),
        Some(&ApprovalScope::Session)
    );
    assert!(
        mode.history_lines()
            .iter()
            .all(|line| !line.contains("[permissions: /edit task grants")),
        "/edit must not announce a task grant when the capability is already session-scoped"
    );
}

#[test]
fn test_fix_command_grants_task_permissions() {
    let mut mode = TuiMode::new();
    let mut edit_loop = EditLoop::new("task-1".to_string());
    edit_loop.set_last_validation_result(crate::runtime::ValidationResult {
        passed: false,
        outputs: vec![crate::runtime::ValidationOutput {
            label: "cargo test".to_string(),
            exit_code: 1,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            stdout_tail_limited: false,
            stderr_tail_limited: false,
        }],
    });
    mode.active_edit_loop = Some(edit_loop);
    let mut ctx = setup_ctx();

    mode.on_user_input("/fix".to_string(), &mut ctx);

    assert_eq!(
        mode.task_doc.info.active_grants.get(&Capability::WriteFile),
        Some(&ApprovalScope::Task)
    );
    assert_eq!(
        mode.task_doc
            .info
            .active_grants
            .get(&Capability::ApplyPatch),
        Some(&ApprovalScope::Task)
    );
    assert_eq!(
        mode.task_doc
            .info
            .active_grants
            .get(&Capability::RunCommand),
        Some(&ApprovalScope::Task)
    );
}

#[test]
fn test_fix_command_preserves_session_permissions() {
    let mut mode = TuiMode::new();
    mode.task_doc
        .info
        .active_grants
        .insert(Capability::WriteFile, ApprovalScope::Session);
    mode.task_doc
        .info
        .active_grants
        .insert(Capability::ApplyPatch, ApprovalScope::Session);
    mode.task_doc
        .info
        .active_grants
        .insert(Capability::RunCommand, ApprovalScope::Session);
    let mut edit_loop = EditLoop::new("task-1".to_string());
    edit_loop.set_last_validation_result(crate::runtime::ValidationResult {
        passed: false,
        outputs: vec![crate::runtime::ValidationOutput {
            label: "cargo test".to_string(),
            exit_code: 1,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            stdout_tail_limited: false,
            stderr_tail_limited: false,
        }],
    });
    mode.active_edit_loop = Some(edit_loop);
    let mut ctx = setup_ctx();

    mode.on_user_input("/fix".to_string(), &mut ctx);

    assert_eq!(
        mode.task_doc.info.active_grants.get(&Capability::WriteFile),
        Some(&ApprovalScope::Session)
    );
    assert_eq!(
        mode.task_doc
            .info
            .active_grants
            .get(&Capability::ApplyPatch),
        Some(&ApprovalScope::Session)
    );
    assert_eq!(
        mode.task_doc
            .info
            .active_grants
            .get(&Capability::RunCommand),
        Some(&ApprovalScope::Session)
    );
    assert!(
        mode.history_lines()
            .iter()
            .all(|line| !line.contains("[permissions: /fix task grants")),
        "/fix must not announce a task grant when the capability is already session-scoped"
    );
}
