use super::*;

#[test]
fn waiting_indicator_appears_on_turn_start_and_clears_on_stream_delta() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("read the release workflow".to_string(), &mut ctx);
    assert!(mode.history_lines().iter().any(|l| l == "[thinking] Mapping adjacent sectors..."));

    mode.on_model_update(UiUpdate::StreamDelta("Hello".to_string()), &mut ctx);
    assert!(!mode.history_lines().iter().any(|l| l.contains("[thinking] Mapping adjacent sectors...")));
    assert!(mode.history_lines().iter().any(|l| l.contains("Hello")));
}

#[test]
fn waiting_indicator_cleared_when_tool_block_starts() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("test prompt".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::StreamBlockStart {
        index: 0,
        block: StreamBlock::ToolCall {
            id: "tc-1".to_string(),
            name: "list_files".to_string(),
            input: serde_json::json!({}),
            status: crate::state::ToolStatus::Executing,
        },
    }, &mut ctx);
    assert!(!mode.history_lines().iter().any(|l| l == "[thinking] Mapping adjacent sectors..."));
}

#[test]
fn duplicate_tool_calls_fold_to_repeated_indicator() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("run check".to_string(), &mut ctx);

    for id in ["tc-1", "tc-2"] {
        mode.on_model_update(UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: id.to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "src/main.rs"}),
                status: crate::state::ToolStatus::Complete,
            },
        }, &mut ctx);
    }

    let lines = mode.history_lines();
    let repeated = lines.iter().any(|l| l.contains("×") || l.contains("repeated") || l.contains("again"));
    assert!(repeated, "duplicate tool calls must fold to a repeated indicator; lines: {lines:?}");
}

#[test]
fn verb_first_read_file_renders_path_in_label() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("read a file".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::StreamBlockStart {
        index: 0,
        block: StreamBlock::ToolCall {
            id: "tc-read".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({"path": "src/lib.rs"}),
            status: crate::state::ToolStatus::Executing,
        },
    }, &mut ctx);
    let lines = mode.history_lines();
    assert!(lines.iter().any(|l| l.contains("src/lib.rs")), "tool call label must include file path; lines: {lines:?}");
}
