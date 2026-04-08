use std::collections::HashMap;

use crate::runtime::json_handoff::RuntimeEvent;
use crate::runtime::task_state::TaskStatus;
use crate::runtime::{ApprovalScope, Capability, ModelBackendKind};
use crate::state::{ToolStatus, TurnToolPolicy};
use crate::usage::TurnTokens;

use super::{TaskDocumentReducer, TaskMeta, TurnEntry, TurnOutcome};

fn test_meta() -> TaskMeta {
    TaskMeta {
        id: "test-task-01".to_string(),
        status: TaskStatus::Ready,
        parent_task_id: None,
        agent_id: None,
        worktree_path: None,
        branch_name: None,
        instructions_path: None,
        model_name: "test-model".to_string(),
        model_backend: ModelBackendKind::LocalRuntime,
        model_url: "https://api.example.com".to_string(),
        started_at_ms: Some(1000),
        updated_at_ms: 1000,
        last_heartbeat_ms: None,
        active_grants: HashMap::new(),
        next_step_id: 1,
    }
}

#[test]
fn begin_task_produces_empty_document() {
    let reducer = TaskDocumentReducer::new();
    let doc = reducer.begin_task(test_meta());
    assert!(doc.completed_turns.is_empty());
    assert!(doc.active_turn.is_none());
    assert_eq!(doc.meta.id, "test-task-01");
}

#[test]
fn begin_turn_opens_active_turn_with_user_input_entry() {
    let reducer = TaskDocumentReducer::new();
    let mut doc = reducer.begin_task(test_meta());

    reducer.begin_turn(
        &mut doc,
        "analyze the test output".to_string(),
        2000,
        TurnToolPolicy::Default,
    );

    let active = doc.active_turn.as_ref().expect("active turn");
    assert_eq!(active.turn_index, 0);
    assert_eq!(active.input, "analyze the test output");
    assert_eq!(active.entries.len(), 1);
    assert!(matches!(active.entries[0], TurnEntry::UserInput { .. }));
}

#[test]
fn finish_turn_moves_active_turn_to_completed() {
    let reducer = TaskDocumentReducer::new();
    let mut doc = reducer.begin_task(test_meta());

    reducer.begin_turn(&mut doc, "q".to_string(), 1000, TurnToolPolicy::Default);
    let summary = reducer.finish_turn(
        &mut doc,
        TurnOutcome::Completed,
        TurnTokens::default(),
        2000,
    );

    assert!(doc.active_turn.is_none());
    assert_eq!(doc.completed_turns.len(), 1);
    assert!(summary.active_turn_changed);
    assert!(summary.task_status_changed);
    assert_eq!(doc.meta.status, TaskStatus::Ready);
}

#[test]
fn apply_tool_call_event_appends_entry() {
    let reducer = TaskDocumentReducer::new();
    let mut doc = reducer.begin_task(test_meta());

    reducer.begin_turn(&mut doc, "q".to_string(), 1000, TurnToolPolicy::Default);
    let summary = reducer.apply_runtime_event(
        &mut doc,
        RuntimeEvent::ToolCall {
            id: "tc-01".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "src/main.rs"}),
        },
    );

    let active = doc.active_turn.as_ref().expect("active turn");
    assert!(active
        .entries
        .iter()
        .any(|entry| matches!(entry, TurnEntry::ToolCall { .. })));
    assert!(summary.active_turn_changed);
}

#[test]
fn tool_result_advances_tool_call_status() {
    let reducer = TaskDocumentReducer::new();
    let mut doc = reducer.begin_task(test_meta());

    reducer.begin_turn(&mut doc, "q".to_string(), 1000, TurnToolPolicy::Default);
    reducer.apply_runtime_event(
        &mut doc,
        RuntimeEvent::ToolCall {
            id: "tc-01".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({}),
        },
    );
    reducer.apply_runtime_event(
        &mut doc,
        RuntimeEvent::ToolResult {
            tool_call_id: "tc-01".to_string(),
            tool_name: Some("read_file".to_string()),
            is_error: false,
            output: "file contents".to_string(),
        },
    );

    let active = doc.active_turn.as_ref().expect("active turn");
    let call_status = active.entries.iter().find_map(|entry| {
        if let TurnEntry::ToolCall { id, status, .. } = entry {
            if id == "tc-01" {
                return Some(status.clone());
            }
        }
        None
    });
    assert_eq!(call_status, Some(ToolStatus::Complete));
}

#[test]
fn snapshot_roundtrip_preserves_turn_count() {
    let reducer = TaskDocumentReducer::new();
    let mut doc = reducer.begin_task(test_meta());

    reducer.begin_turn(
        &mut doc,
        "turn one".to_string(),
        1000,
        TurnToolPolicy::Default,
    );
    reducer.finish_turn(
        &mut doc,
        TurnOutcome::Completed,
        TurnTokens::default(),
        2000,
    );
    reducer.begin_turn(
        &mut doc,
        "turn two".to_string(),
        3000,
        TurnToolPolicy::Default,
    );
    reducer.finish_turn(
        &mut doc,
        TurnOutcome::Completed,
        TurnTokens::default(),
        4000,
    );

    let snapshot = reducer.persistable_snapshot(&doc);
    assert_eq!(snapshot.turns.len(), 2);

    let restored = reducer.restore_from_snapshot(snapshot);
    assert_eq!(restored.completed_turns.len(), 2);
}

#[test]
fn error_event_sets_error_state_and_status() {
    let reducer = TaskDocumentReducer::new();
    let mut doc = reducer.begin_task(test_meta());

    let summary = reducer.apply_runtime_event(
        &mut doc,
        RuntimeEvent::Error {
            code: "E001".to_string(),
            message: "something went wrong".to_string(),
            recoverable: false,
        },
    );

    assert!(doc.last_error.is_some());
    assert_eq!(doc.meta.status, TaskStatus::Failed);
    assert!(summary.task_status_changed);
}

#[test]
fn hyphenated_capability_names_parse() {
    assert_eq!("read-file".parse::<Capability>(), Ok(Capability::ReadFile));
    assert_eq!(
        "apply-patch".parse::<Capability>(),
        Ok(Capability::ApplyPatch)
    );
    assert_eq!(
        "run-command".parse::<Capability>(),
        Ok(Capability::RunCommand)
    );
}

#[test]
fn approval_resolution_updates_grants_by_scope() {
    let reducer = TaskDocumentReducer::new();
    let mut doc = reducer.begin_task(test_meta());

    reducer.begin_turn(&mut doc, "q".to_string(), 1000, TurnToolPolicy::Default);

    reducer.apply_runtime_event(
        &mut doc,
        RuntimeEvent::ApprovalResolved {
            capability: "apply-patch".to_string(),
            scope: "session".to_string(),
            approved: true,
        },
    );
    assert_eq!(
        doc.meta.active_grants.get(&Capability::ApplyPatch),
        Some(&ApprovalScope::Session)
    );

    reducer.apply_runtime_event(
        &mut doc,
        RuntimeEvent::ApprovalResolved {
            capability: "run-command".to_string(),
            scope: "once".to_string(),
            approved: true,
        },
    );
    assert_eq!(doc.meta.active_grants.get(&Capability::RunCommand), None);
}

#[test]
fn snapshot_preserves_denied_tool_outcome_on_restore() {
    let reducer = TaskDocumentReducer::new();
    let mut doc = reducer.begin_task(test_meta());

    reducer.begin_turn(&mut doc, "q".to_string(), 1000, TurnToolPolicy::Default);
    reducer.apply_runtime_event(
        &mut doc,
        RuntimeEvent::ToolCall {
            id: "tc-01".to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({"path": "src/main.rs"}),
        },
    );
    reducer.apply_runtime_event(
        &mut doc,
        RuntimeEvent::ToolResult {
            tool_call_id: "tc-01".to_string(),
            tool_name: Some("write_file".to_string()),
            is_error: true,
            output: "permission denied".to_string(),
        },
    );
    reducer.finish_turn(
        &mut doc,
        TurnOutcome::Completed,
        TurnTokens::default(),
        2000,
    );

    let snapshot = reducer.persistable_snapshot(&doc);
    assert_eq!(snapshot.turns[0].tool_invocations[0].outcome, "denied");

    let restored = reducer.restore_from_snapshot(snapshot);
    let restored_status = restored.completed_turns[0]
        .entries
        .iter()
        .find_map(|entry| {
            if let TurnEntry::ToolCall { status, .. } = entry {
                return Some(status.clone());
            }
            None
        });
    assert_eq!(restored_status, Some(ToolStatus::Error));
}
