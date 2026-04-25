use super::*;

#[test]
fn permissions_command_lists_all_capabilities_with_grants() {
    let mut mode = TuiMode::new();
    mode.task_doc.info.active_grants.insert(Capability::RunCommand, ApprovalScope::Session);
    mode.task_doc.info.active_grants.insert(Capability::Network, ApprovalScope::Once);
    let mut ctx = setup_ctx();
    mode.on_user_input("/permissions".to_string(), &mut ctx);
    let lines = mode.history_lines().to_vec();
    assert!(lines.iter().any(|l| l == "[permissions]"));
    assert!(lines.iter().any(|l| l.contains("run-command") && l.contains("session")));
    assert!(lines.iter().any(|l| l.contains("network") && l.contains("once")));
    assert!(lines.iter().any(|l| l.contains("apply-patch") && l.contains("(none)")));
    assert!(!mode.is_pulse_in_progress());
}

#[test]
fn allow_command_inserts_grant_and_validates_scope_and_capability() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    // valid session grant
    mode.on_user_input("/allow run-command session".to_string(), &mut ctx);
    assert_eq!(mode.task_doc.info.active_grants.get(&Capability::RunCommand), Some(&ApprovalScope::Session));
    assert!(mode.history_lines().iter().any(|l| l.contains("run-command granted for session")));

    // default to once
    mode.on_user_input("/allow write-file".to_string(), &mut ctx);
    assert_eq!(mode.task_doc.info.active_grants.get(&Capability::WriteFile), Some(&ApprovalScope::Once));

    // unknown capability
    mode.on_user_input("/allow bogus-cap".to_string(), &mut ctx);
    assert!(mode.history_lines().iter().any(|l| l.contains("unknown capability 'bogus-cap'")));

    // invalid scope
    mode.on_user_input("/allow network task".to_string(), &mut ctx);
    assert!(mode.history_lines().iter().any(|l| l.contains("unknown scope 'task'")));
}
