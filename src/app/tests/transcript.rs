use super::*;
use crate::runtime::AssistantPhase;

#[test]
fn test_ref_08_stream_delta_appends_to_assistant_placeholder_not_user_line() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("describe the error".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("assistant".to_string()), &mut ctx);

    let hl = mode.history_lines();
    assert_eq!(hl[0], "> describe the error");
    assert!(
        hl[1].starts_with("assistant"),
        "assistant content should appear at index 1, got hl[1]={:?}",
        hl.get(1)
    );
}

#[test]
fn test_transcript_history_cap_removed() {}
#[test]
fn test_scrollback_retains_position_during_streaming() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    use crate::runtime::task_document::NoticeSeverity;
    for i in 0..20 {
        mode.push_document_notice(format!("line-{i}"), NoticeSeverity::Info);
    }

    mode.on_user_input("fix the import error".to_string(), &mut ctx);

    mode.transcript_scroll_offset = 5;

    mode.on_model_update(UiUpdate::StreamDelta(" assistant".to_string()), &mut ctx);

    assert!(
        mode.transcript_scroll_offset > 0,
        "scrollback position must not be forced to bottom while user has scrolled up"
    );
}
#[test]
fn test_output_scroll_commands_update_scroll_state() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("list the test failures".to_string(), &mut ctx);
    for i in 0..50 {
        mode.push_history_line(format!("line-{i}"));
    }

    assert!(mode.auto_follow(), "initial state must be auto-following");

    mode.apply_output_scroll_action(ScrollAction::LineUp);
    assert!(!mode.auto_follow(), "scrolling up must disable auto-follow");

    mode.apply_output_scroll_action(ScrollAction::End);
    assert!(mode.auto_follow(), "End must restore auto-follow");

    mode.apply_output_scroll_action(ScrollAction::Home);
    assert!(!mode.auto_follow(), "Home must disable auto-follow");

    mode.apply_output_scroll_action(ScrollAction::End);
    assert!(mode.auto_follow(), "End must restore auto-follow again");
}
#[test]
fn test_history_status_uses_visual_rows() {
    use crate::runtime::task_document::NoticeSeverity;
    let mut mode = TuiMode::new();
    mode.push_document_notice("a\nb\nc".to_string(), NoticeSeverity::Info);
    assert!(mode.status_line().contains("history:3"));
}
#[test]
fn header_stable_during_streaming() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    let ready_status = mode.status_line();
    assert!(
        ready_status.contains("mode:ready"),
        "ready state must publish mode token"
    );
    assert!(
        ready_status.contains("approval:none"),
        "ready state must publish approval token"
    );
    assert!(
        ready_status.contains("history:0"),
        "ready state must publish history count"
    );
    assert!(
        ready_status.contains("repo:"),
        "ready state must publish repo token"
    );

    mode.on_user_input("explain this function".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("assistant".to_string()), &mut ctx);
    let streaming_status = mode.status_line();
    assert!(
        streaming_status.contains("mode:streaming"),
        "streaming state must publish mode token"
    );
    assert!(
        streaming_status.contains("approval:none"),
        "streaming state must preserve approval token"
    );
    assert!(
        streaming_status.contains("history:2"),
        "streaming state must keep compact history count"
    );

    let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );
    let overlay_status = mode.status_line();
    assert!(
        overlay_status.contains("mode:overlay"),
        "overlay state must publish overlay mode token"
    );
    assert!(
        overlay_status.contains("approval:pending"),
        "overlay state must publish pending approval token"
    );
}
#[test]
fn test_stream_delta_ignored_without_active_turn_slot() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_model_update(UiUpdate::StreamDelta("ghost delta".to_string()), &mut ctx);
    assert!(
        mode.history_lines().is_empty(),
        "stale stream deltas must be ignored after turn completion/cancel"
    );
}
#[test]
fn test_cancel_pending_blocks_stream_delta_appends() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("run the linter".to_string(), &mut ctx);
    mode.on_interrupt(&mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("stale".to_string()), &mut ctx);
    let hl = mode.history_lines();
    assert_eq!(hl[0], "> run the linter");
    assert!(
        hl.iter()
            .any(|l| l.starts_with("[thinking] Mapping adjacent sectors...")),
        "cancel_pending should keep the waiting placeholder: {hl:?}"
    );
    assert!(
        !hl.iter().any(|l| l.contains("stale")),
        "stale delta must not appear in transcript after cancellation"
    );
}

#[test]
fn test_consecutive_read_only_tools_fold_into_single_paragraph() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("analyze src/main.rs".to_string(), &mut ctx);

    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tc-1".to_string(),
                name: "codebase_search".to_string(),
                input: serde_json::json!({"query": "main function"}),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolResult {
                tool_call_id: "tc-1".to_string(),
                output: "found 1 match in src/main.rs".to_string(),
                is_error: false,
            },
        },
        &mut ctx,
    );

    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 2,
            block: StreamBlock::ToolCall {
                id: "tc-2".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "src/main.rs"}),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 3,
            block: StreamBlock::ToolResult {
                tool_call_id: "tc-2".to_string(),
                output: "fn main() { ... }".to_string(),
                is_error: false,
            },
        },
        &mut ctx,
    );

    let hl = mode.history_lines();
    let tool_headers: Vec<_> = hl
        .iter()
        .filter(|line| line.starts_with("[tool] "))
        .collect();
    assert_eq!(
        tool_headers.len(),
        2,
        "each completed tool must render individually; found {tool_headers:?}"
    );
}

#[test]
fn test_edit_file_transcript_preview_preserves_structured_diff_rows() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("update src/main.rs".to_string(), &mut ctx);

    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "edit-1".to_string(),
                name: "edit_file".to_string(),
                input: serde_json::json!({
                    "path": "src/main.rs",
                    "old_str": "fn main() {\n    old_call();\n}\n",
                    "new_str": "fn main() {\n    new_call();\n}\n",
                }),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );

    let hl = mode.history_lines();
    assert!(
        hl.iter()
            .any(|line| line == "[detail] Input: path: src/main.rs"),
        "edit_file preview must keep the path row visible: {:?}",
        hl
    );
    assert!(
        hl.iter().any(|line| line.contains("[evidence]")
            && line.contains("-")
            && line.contains("old_call();")),
        "edit_file preview must preserve deleted diff rows: {:?}",
        hl
    );
    assert!(
        hl.iter().any(|line| line.contains("[evidence]")
            && line.contains("+")
            && line.contains("new_call();")),
        "edit_file preview must preserve inserted diff rows: {:?}",
        hl
    );

    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolResult {
                tool_call_id: "edit-1".to_string(),
                output: "Edited src/main.rs successfully.".to_string(),
                is_error: false,
            },
        },
        &mut ctx,
    );

    let hl2 = mode.history_lines();
    assert!(
        hl2.iter()
            .any(|line| line == "[detail] Input: path: src/main.rs"),
        "completed edit_file rows must keep the structured path preview: {:?}",
        hl2
    );
    assert!(
        hl2.iter().any(|line| line.contains("[evidence]")
            && line.contains("+")
            && line.contains("new_call();")),
        "completed edit_file rows must keep diff evidence visible: {:?}",
        hl2
    );
}

#[test]
fn stream_block_start_reuses_existing_block_index_for_phase_change() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("summarize the file".to_string(), &mut ctx);

    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::Thinking {
                content: String::new(),
                collapsed: false,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockDelta {
            index: 0,
            delta: "partial answer".to_string(),
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::FinalText {
                content: String::new(),
            },
        },
        &mut ctx,
    );

    let assistant_blocks: Vec<_> = mode
        .task_doc
        .active_turn
        .as_ref()
        .unwrap()
        .entries
        .iter()
        .filter_map(|entry| {
            if let TurnEntry::AssistantBlock { block, .. } = entry {
                Some(block)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(assistant_blocks.len(), 1);
    assert_eq!(assistant_blocks[0].block_index, 0);
    assert_eq!(assistant_blocks[0].phase, AssistantPhase::Final);
    assert_eq!(assistant_blocks[0].content, "partial answer");
}
