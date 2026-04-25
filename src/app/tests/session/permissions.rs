use super::*;

#[test]
fn test_permissions_empty_grants() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/permissions".to_string(), &mut ctx);
    assert!(
        mode.history_lines().iter().any(|l| l == "[permissions]"),
        "expected permissions header"
    );
    for &cap in ALL_CAPABILITIES {
        let cap_name = capability_to_kebab(cap);
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains(cap_name) && l.contains("(none)")),
            "expected {cap_name} with (none) in empty-grants permissions output"
        );
    }
    assert!(!mode.is_pulse_in_progress());
}

#[test]
fn test_permissions_lists_active_grants() {
    let mut mode = TuiMode::new();
    mode.task_doc
        .info
        .active_grants
        .insert(Capability::RunCommand, ApprovalScope::Session);
    mode.task_doc
        .info
        .active_grants
        .insert(Capability::Network, ApprovalScope::Once);
    let mut ctx = setup_ctx();
    mode.on_user_input("/permissions".to_string(), &mut ctx);
    let lines = mode.history_lines().to_vec();
    let has_header = lines.iter().any(|l| l == "[permissions]");
    let has_run_command = lines
        .iter()
        .any(|l| l.contains("run-command") && l.contains("session"));
    let has_network = lines
        .iter()
        .any(|l| l.contains("network") && l.contains("once"));
    let has_apply_patch_none = lines
        .iter()
        .any(|l| l.contains("apply-patch") && l.contains("(none)"));
    assert!(has_header, "expected active grants header");
    assert!(has_run_command, "expected run-command session entry");
    assert!(has_network, "expected network once entry");
    assert!(
        has_apply_patch_none,
        "expected apply-patch (none) for absent grant"
    );
    assert!(!mode.is_pulse_in_progress());
}

#[test]
fn test_allow_inserts_grant() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/allow run-command session".to_string(), &mut ctx);
    assert_eq!(
        mode.task_doc
            .info
            .active_grants
            .get(&Capability::RunCommand),
        Some(&ApprovalScope::Session),
        "allow must insert the grant with session scope"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[allow: run-command granted for session]")),
        "expected grant confirmation"
    );
    assert!(!mode.is_pulse_in_progress());
}

#[test]
fn test_allow_defaults_to_once_scope() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/allow write-file".to_string(), &mut ctx);
    assert_eq!(
        mode.task_doc.info.active_grants.get(&Capability::WriteFile),
        Some(&ApprovalScope::Once),
        "allow without scope must default to once"
    );
}

#[test]
fn test_allow_unknown_capability_emits_error() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/allow bogus-cap".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[allow: unknown capability 'bogus-cap']")),
        "expected unknown-capability error"
    );
    assert!(mode.task_doc.info.active_grants.is_empty());
    assert!(!mode.is_pulse_in_progress());
}

#[test]
fn test_allow_task_scope_emits_error() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/allow network task".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[allow: unknown scope 'task'; valid: once | session]")),
        "expected task scope rejection"
    );
    assert!(mode.task_doc.info.active_grants.is_empty());
    assert!(!mode.is_pulse_in_progress());
}

#[test]
fn test_allow_unknown_scope_emits_error() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/allow network forever".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[allow: unknown scope 'forever'; valid: once | session]")),
        "expected unknown-scope error"
    );
    assert!(mode.task_doc.info.active_grants.is_empty());
    assert!(!mode.is_pulse_in_progress());
}

#[test]
fn test_deny_removes_grant() {
    let mut mode = TuiMode::new();
    mode.task_doc
        .info
        .active_grants
        .insert(Capability::ApplyPatch, ApprovalScope::Task);
    let mut ctx = setup_ctx();
    mode.on_user_input("/deny apply-patch".to_string(), &mut ctx);
    assert!(
        !mode
            .task_doc
            .info
            .active_grants
            .contains_key(&Capability::ApplyPatch),
        "deny must remove the grant"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[deny: apply-patch removed]")),
        "expected revoke confirmation"
    );
    assert!(!mode.is_pulse_in_progress());
}

#[test]
fn test_deny_no_grant_emits_info() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/deny browser".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[deny: browser not in active grants]")),
        "expected no-active-grant info message"
    );
    assert!(!mode.is_pulse_in_progress());
}

#[test]
fn test_deny_unknown_capability_emits_error() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/deny not-a-thing".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[deny: unknown capability 'not-a-thing']")),
        "expected unknown-capability error"
    );
    assert!(!mode.is_pulse_in_progress());
}

#[test]
fn test_capability_kebab_round_trip() {
    for &cap in ALL_CAPABILITIES {
        let kebab = capability_to_kebab(cap);
        let round_tripped = kebab_to_capability(kebab);
        assert_eq!(
            round_tripped,
            Some(cap),
            "capability {kebab} failed round-trip through kebab_to_capability"
        );
    }
}

#[test]
fn test_capability_for_tool_name_maps_builtin_tools() {
    assert_eq!(
        capability_for_tool_name("read_file"),
        Some(Capability::ReadFile)
    );
    assert_eq!(
        capability_for_tool_name("write_file"),
        Some(Capability::WriteFile)
    );
    assert_eq!(
        capability_for_tool_name("apply_patch"),
        Some(Capability::ApplyPatch)
    );
    assert_eq!(
        capability_for_tool_name("run_command"),
        Some(Capability::RunCommand)
    );
    assert_eq!(
        capability_for_tool_name("git_commit"),
        Some(Capability::ApplyPatch)
    );
    assert_eq!(capability_for_tool_name("unknown_tool"), None);
}
