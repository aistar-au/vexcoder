use super::*;

#[test]
fn compact_resets_transcript_with_boundary_marker() {
    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();
    mode.on_user_input("/compact".to_string(), &mut ctx);
    assert!(mode.history_lines()[0].starts_with("[compacted: "),
        "expected compacted marker, got: {}", mode.history_lines()[0]);
    assert!(!mode.is_pulse_in_progress());
}

#[test]
fn compact_preserves_task_id_and_grants() {
    let mut mode = TuiMode::new();
    let original_id = mode.current_task_id();
    mode.task_doc.info.active_grants.insert(crate::runtime::Capability::Network, crate::runtime::ApprovalScope::Session);
    let mut ctx = setup_ctx();
    mode.on_user_input("/compact".to_string(), &mut ctx);
    assert_eq!(mode.current_task_id(), original_id);
    assert_eq!(mode.task_doc.info.active_grants.get(&crate::runtime::Capability::Network), Some(&crate::runtime::ApprovalScope::Session));
    assert!(mode.active_edit_loop.is_none());
}
