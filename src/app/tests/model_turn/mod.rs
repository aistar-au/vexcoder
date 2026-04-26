use super::*;

mod permissions;
mod shell_execution;
mod slash_commands;
mod tool_approval;
mod tool_rendering;

#[test]
fn tool_result_records_changed_files_only_on_success() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("task".to_string(), &mut ctx);

    let tool_call = StreamBlock::ToolCall {
        id: "tool-1".to_string(),
        name: "write_file".to_string(),
        input: serde_json::json!({"path": "src/main.rs", "content": "fn main() {}\n"}),
        status: crate::state::ToolStatus::Executing,
    };
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: tool_call,
        },
        &mut ctx,
    );
    assert!(
        mode.task_doc
            .active_pulse
            .as_ref()
            .is_none_or(|t| t.changed_files.is_empty()),
        "tool calls must not record changed files until they succeed"
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
    assert!(
        mode.task_doc
            .active_pulse
            .as_ref()
            .is_some_and(|t| t.changed_files.contains("src/main.rs"))
    );

    // failed tool must not record
    let mut mode2 = TuiMode::new();
    let mut ctx2 = setup_ctx();
    mode2.on_user_input("task".to_string(), &mut ctx2);
    mode2.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tool-2".to_string(),
                name: "write_file".to_string(),
                input: serde_json::json!({"path": "src/main.rs", "content": ""}),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx2,
    );
    mode2.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolResult {
                tool_call_id: "tool-2".to_string(),
                output: "permission denied".to_string(),
                is_error: true,
            },
        },
        &mut ctx2,
    );
    assert!(
        mode2
            .task_doc
            .active_pulse
            .as_ref()
            .is_none_or(|t| t.changed_files.is_empty()),
        "failed tool must not record changed files"
    );
}

#[test]
fn error_reset_clears_changed_files_and_records_single_error_marker() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_model_update(UiUpdate::Error("reset".to_string()), &mut ctx);
    assert!(
        mode.task_layout_state()
            .expect("layout")
            .changed_files
            .is_empty()
    );

    let mut mode2 = TuiMode::new();
    let mut ctx2 = setup_ctx();
    mode2.on_user_input("summarize the repository".to_string(), &mut ctx2);
    mode2.on_model_update(UiUpdate::Error("transport timeout".to_string()), &mut ctx2);
    let lines = mode2.history_lines();
    assert!(lines.iter().any(|l| l == "[error] transport timeout"));
    assert!(
        !lines
            .iter()
            .any(|l| l == "[error] [error] transport timeout"),
        "must not duplicate error"
    );
}

#[test]
fn idle_interrupt_arms_quit_then_fires_on_second_interrupt() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_interrupt(&mut ctx);
    assert!(mode.pending_quit, "first idle interrupt must arm quit");
    mode.on_interrupt(&mut ctx);
    assert!(
        mode.quit_requested,
        "second idle interrupt must trigger quit"
    );
}
