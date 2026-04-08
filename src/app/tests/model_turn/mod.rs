use super::*;

mod permissions;
mod shell_execution;
mod slash_commands;
mod tool_approval;
mod tool_rendering;

// -- changed-file tracking ---------------------------------------------------

#[test]
fn tool_call_only_marks_changed_files_after_successful_result() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("task".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tool-1".to_string(),
                name: "write_file".to_string(),
                input: serde_json::json!({
                    "path": "src/main.rs",
                    "content": "fn main() {}\n"
                }),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );
    assert!(
        mode.task_doc
            .active_turn
            .as_ref()
            .is_none_or(|t| t.changed_files.is_empty()),
        "tool calls should not record changed files until they succeed"
    );

    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolResult {
                tool_call_id: "tool-1".to_string(),
                output: "ok".to_string(),
                is_error: false,
            },
        },
        &mut ctx,
    );
    assert!(mode
        .task_doc
        .active_turn
        .as_ref()
        .is_some_and(|t| t.changed_files.contains("src/main.rs")));

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(state.changed_files, vec!["src/main.rs".to_string()]);
}

#[test]
fn failed_tool_result_does_not_record_changed_files() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("task".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tool-1".to_string(),
                name: "write_file".to_string(),
                input: serde_json::json!({
                    "path": "src/main.rs",
                    "content": "fn main() {}\n"
                }),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolResult {
                tool_call_id: "tool-1".to_string(),
                output: "permission denied".to_string(),
                is_error: true,
            },
        },
        &mut ctx,
    );
    assert!(
        mode.task_doc
            .active_turn
            .as_ref()
            .is_none_or(|t| t.changed_files.is_empty()),
        "failed tool calls must not be exported as changed files"
    );
}

#[test]
fn error_reset_clears_live_changed_file_projection() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    // In the new document model in-flight changed files only exist inside an
    // active turn.  Without a live turn the projection is always empty.

    mode.on_model_update(UiUpdate::Error("reset".to_string()), &mut ctx);

    let state = mode.task_layout_state().expect("task layout state");
    assert!(
        state.changed_files.is_empty(),
        "error reset should clear in-flight changed file projection"
    );
}

// -- interrupt / feedback / quit ---------------------------------------------

#[test]
fn test_idle_interrupt_shows_feedback() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    assert!(mode.task_doc.active_turn.is_none());
    assert!(!mode.pending_quit);
    assert!(!mode.quit_requested);

    mode.on_interrupt(&mut ctx);
    assert!(mode.pending_quit, "first idle interrupt must arm quit");
    assert!(!mode.quit_requested, "first idle interrupt must not quit");
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("[press Ctrl+C again to exit]")),
        "first idle interrupt must show user-visible feedback"
    );

    mode.on_interrupt(&mut ctx);
    assert!(
        mode.quit_requested,
        "second idle interrupt must request quit"
    );
    assert!(
        mode.quit_requested(),
        "frontend quit path must observe mode quit request"
    );
}

#[test]
fn test_input_drop_shows_feedback() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.begin_turn_capture("test".to_string());
    mode.on_user_input("analyze the test output".to_string(), &mut ctx);

    assert!(
        mode.task_doc.active_turn.is_some(),
        "busy input must not start a new turn"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.starts_with("[busy")),
        "busy input must produce visible rejection feedback"
    );
    assert!(
        !mode
            .history_lines()
            .iter()
            .any(|line| line == "> analyze the test output"),
        "discarded busy input must not be appended as user message"
    );
}

#[test]
fn test_pending_quit_resets_on_new_turn_accept() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_interrupt(&mut ctx);
    assert!(mode.pending_quit);

    mode.on_user_input("resume".to_string(), &mut ctx);
    assert!(
        !mode.pending_quit,
        "pending quit must reset when a new turn is accepted"
    );
    assert!(!mode.quit_requested);
    assert!(mode.task_doc.active_turn.is_some());
}

#[tokio::test]
async fn test_interrupt_is_typed_event_not_magic_string_collision() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();

    mode.on_user_input("__VEX_INTERRUPT__".to_string(), &mut ctx);
    assert!(
        mode.task_doc.active_turn.is_some(),
        "plain text matching old sentinel must be treated as normal user input"
    );

    mode.on_interrupt(&mut ctx);
    assert!(
        mode.task_doc.active_turn.is_some(),
        "typed interrupt should keep turn active until TurnComplete drains"
    );
    assert!(
        mode.task_doc
            .active_turn
            .as_ref()
            .is_some_and(|t| t.cancel_pending),
        "typed interrupt should arm cancel-pending state"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("[turn cancellation requested]")),
        "cancel path should provide visible feedback"
    );

    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
    assert!(mode.task_doc.active_turn.is_none());
    assert!(!mode
        .task_doc
        .active_turn
        .as_ref()
        .is_some_and(|t| t.cancel_pending));
}
