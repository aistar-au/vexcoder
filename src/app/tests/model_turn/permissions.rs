use super::*;

#[test]
fn edit_command_grants_task_permissions_and_preserves_session_scope() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/edit refactor src/app.rs".to_string(), &mut ctx);
    assert_eq!(mode.task_doc.info.active_grants.get(&Capability::WriteFile), Some(&ApprovalScope::Task));
    assert_eq!(mode.task_doc.info.active_grants.get(&Capability::ApplyPatch), Some(&ApprovalScope::Task));
    assert_eq!(mode.task_doc.info.active_grants.get(&Capability::RunCommand), Some(&ApprovalScope::Task));

    // session scope must not be downgraded to task scope
    let mut mode2 = TuiMode::new();
    mode2.task_doc.info.active_grants.insert(Capability::WriteFile, ApprovalScope::Session);
    mode2.task_doc.info.active_grants.insert(Capability::RunCommand, ApprovalScope::Session);
    let mut ctx2 = setup_ctx();
    mode2.on_user_input("/edit refactor src/app.rs".to_string(), &mut ctx2);
    assert_eq!(mode2.task_doc.info.active_grants.get(&Capability::WriteFile), Some(&ApprovalScope::Session),
        "session scope must be preserved; must not be downgraded to task");
    assert_eq!(mode2.task_doc.info.active_grants.get(&Capability::RunCommand), Some(&ApprovalScope::Session));
}
