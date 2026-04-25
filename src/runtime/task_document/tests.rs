use std::collections::HashMap;

use crate::runtime::json_handoff::RuntimeEvent;
use crate::runtime::task_state::TaskStatus;
use crate::runtime::{ApprovalScope, Capability, ModelBackendKind};
use crate::state::{PulseToolPolicy, ToolStatus};
use crate::usage::PulseTokens;

use super::{PulseEntry, PulseOutcome, TaskDocumentCondenser, TaskInfo};

fn test_meta() -> TaskInfo {
    TaskInfo {
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
fn task_document_lifecycle_begin_turn_finish_and_tool_tracking() {
    let condenser = TaskDocumentCondenser::new();
    let mut doc = condenser.begin_task(test_meta());
    assert!(doc.completed_turns.is_empty() && doc.active_pulse.is_none());

    condenser.begin_turn(&mut doc, "analyze the output".to_string(), 2000, PulseToolPolicy::Default);
    let active = doc.active_pulse.as_ref().expect("active pulse");
    assert_eq!(active.input, "analyze the output");
    assert!(matches!(active.entries[0], PulseEntry::UserInput { .. }));

    let summary = condenser.finish_turn(&mut doc, PulseOutcome::Completed, PulseTokens::default(), 3000);
    assert!(doc.active_pulse.is_none());
    assert_eq!(doc.completed_turns.len(), 1);
    assert!(summary.active_turn_changed && summary.task_status_changed);
}
