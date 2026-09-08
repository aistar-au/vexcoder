use super::*;
use crate::runtime::{
    AssistantBlockEntry, AssistantPhase, PulseEntry, PulseOutcome, TurnDocument, WorkingSetRecord,
};

#[test]
fn compact_resets_transcript_with_boundary_marker() {
    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();
    mode.on_user_input("/compact".to_string(), &mut ctx);
    assert!(
        mode.history_lines()[0].starts_with("[compacted: "),
        "expected compacted marker, got: {}",
        mode.history_lines()[0]
    );
    assert!(!mode.is_pulse_in_progress());
}

#[test]
fn compact_preserves_task_id_and_grants() {
    let mut mode = TuiMode::new();
    let original_id = mode.current_task_id();
    mode.task_doc.info.active_grants.insert(
        crate::runtime::Capability::Network,
        crate::runtime::ApprovalScope::Session,
    );
    let mut ctx = setup_ctx();
    mode.on_user_input("/compact".to_string(), &mut ctx);
    assert_eq!(mode.current_task_id(), original_id);
    assert_eq!(
        mode.task_doc
            .info
            .active_grants
            .get(&crate::runtime::Capability::Network),
        Some(&crate::runtime::ApprovalScope::Session)
    );
    assert!(mode.active_edit_loop.is_none());
}

fn completed_turn(input: &str, response: &str, changed_file: &str) -> TurnDocument {
    TurnDocument {
        turn_index: 0,
        input: input.to_string(),
        entries: vec![
            PulseEntry::UserInput {
                step_id: 1,
                text: input.to_string(),
            },
            PulseEntry::AssistantBlock {
                step_id: 2,
                block: AssistantBlockEntry {
                    block_index: 0,
                    phase: AssistantPhase::Final,
                    content: response.to_string(),
                    collapsed: false,
                    streaming: false,
                },
            },
        ],
        outcome: PulseOutcome::Completed,
        changed_files: vec![changed_file.to_string()],
        command_history: Vec::new(),
        tokens: crate::usage::PulseTokens::default(),
        started_at_ms: 1,
        completed_at_ms: 2,
        ttft_ms: None,
        timings: None,
    }
}

#[test]
fn compact_writes_working_set_before_clearing_pulses() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let task_id = mode.current_task_id();
    mode.task_doc.completed_turns.push(completed_turn(
        "distinctive compact objective",
        "verified compact result",
        "src/runtime/task_state/working_set.rs",
    ));
    let mut ctx = setup_ctx();
    mode.on_user_input("/compact".to_string(), &mut ctx);

    assert!(
        mode.task_doc.completed_turns.is_empty(),
        "compact must clear completed pulses after writing the record"
    );
    let loaded = WorkingSetRecord::load(temp.path(), &task_id).expect("working-set sidecar");
    assert_eq!(loaded.objective, "distinctive compact objective");
    assert!(
        loaded.changed_paths.iter().any(|change| change
            .path
            .ends_with("src/runtime/task_state/working_set.rs")),
        "record must copy changed_files from pulses that compact later clears; got {:?}",
        loaded.changed_paths
    );
    assert!(
        loaded
            .verified_results
            .iter()
            .any(|result| result.contains("verified compact result")),
        "record must copy assistant final text from pulses; got {:?}",
        loaded.verified_results
    );
    let prompt = ctx
        .test_system_prompt_try_lock()
        .expect("conversation lock");
    assert!(
        prompt.contains("distinctive compact objective"),
        "next request system prompt must contain WorkingSetRecord::as_prompt_block; got {prompt}"
    );
    assert!(prompt.contains("[working-set record: start]"));
    assert_eq!(mode.current_task_id(), task_id);

    crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
}
