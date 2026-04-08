use super::*;

// -- waiting indicators ------------------------------------------------------

#[test]
fn test_waiting_indicator_appears_on_turn_start() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("read the release workflow".to_string(), &mut ctx);

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[thinking] Mapping adjacent sectors..."),
        "turn start must show waiting indicator"
    );
}

#[test]
fn test_waiting_indicator_cleared_on_stream_delta() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("test prompt".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[thinking] Mapping adjacent sectors..."),
        "waiting indicator must be present before first delta"
    );

    mode.on_model_update(UiUpdate::StreamDelta("Hello".to_string()), &mut ctx);

    assert!(
        !mode
            .history_lines()
            .iter()
            .any(|line| line.contains("[thinking] Mapping adjacent sectors...")),
        "waiting indicator must be cleared after first stream delta"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("Hello")),
        "first stream delta content must be visible"
    );
}

#[test]
fn test_waiting_indicator_cleared_on_tool_block() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("test prompt".to_string(), &mut ctx);

    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tc-1".to_string(),
                name: "list_files".to_string(),
                input: serde_json::json!({}),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );

    assert!(
        !mode
            .history_lines()
            .iter()
            .any(|line| line == "[thinking] Mapping adjacent sectors..."),
        "waiting indicator must be cleared when a tool block starts"
    );
}

// -- verb-first patterns -----------------------------------------------------

#[test]
fn test_verb_first_read_file_empty_path() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("read something".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tc-read".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({}),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolResult {
                tool_call_id: "tc-read".to_string(),
                output: "I need an explicit file path".to_string(),
                is_error: true,
            },
        },
        &mut ctx,
    );

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[tool] read_file · failed"),
        "read_file with empty path and error must show paragraph tool header"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("[detail] Result: I need an explicit file path")),
        "read_file with empty path and error must preserve the result summary"
    );
}

#[test]
fn test_verb_first_list_files_empty_path() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("list files".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tc-list".to_string(),
                name: "list_files".to_string(),
                input: serde_json::json!({}),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolResult {
                tool_call_id: "tc-list".to_string(),
                output: "src/main.rs\nCargo.toml\n".to_string(),
                is_error: false,
            },
        },
        &mut ctx,
    );

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.starts_with("[tool] list_files · ")),
        "list_files with empty path must show a paragraph tool header"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[evidence] src/main.rs")
            && mode
                .history_lines()
                .iter()
                .any(|line| line == "[evidence] Cargo.toml"),
        "list_files with empty path must retain the listed workspace evidence"
    );
}

// -- tool block rendering ----------------------------------------------------

#[test]
fn test_tool_blocks_emit_paragraph_rows_into_history() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("inspect the file".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tc-read".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path":"src/main.rs"}),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[tool] read_file · src/main.rs · Mapping adjacent sectors..."),
        "pending tool calls must render into the scrolling transcript"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| { line == "[detail] Input: path: src/main.rs" }),
        "pending tool calls must preserve the compact input preview"
    );

    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolResult {
                tool_call_id: "tc-read".to_string(),
                output: "42 lines read from src/main.rs\nfn main() {}".to_string(),
                is_error: false,
            },
        },
        &mut ctx,
    );

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[tool] read_file · src/main.rs · Response complete."),
        "completed tool calls must render their terminal paragraph header"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[evidence] fn main() {}"),
        "completed tool calls must keep enriched evidence in the transcript"
    );
}

// -- streaming deltas --------------------------------------------------------

#[test]
fn test_stream_block_delta_updates_pending_tool_call_input() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.begin_turn_capture("test".to_string());

    // Start a ToolCall block with empty input.
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tc1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::Value::Object(Default::default()),
                status: crate::state::ToolStatus::Pending,
            },
        },
        &mut ctx,
    );

    // In the new document model, pending tool calls are stored as TurnEntry::ToolCall
    // inside active_turn.entries rather than in a separate pending map.
    assert!(
        mode.task_doc.active_turn.as_ref().is_some_and(|t| {
            t.entries.iter().any(|e| {
                matches!(e,
                    crate::runtime::TurnEntry::ToolCall { id, .. } if id == "tc1"
                )
            })
        }),
        "StreamBlockStart must register pending tool call in active turn entries"
    );

    // First partial delta — not yet valid JSON.
    mode.on_model_update(
        UiUpdate::StreamBlockDelta {
            index: 0,
            delta: r#"{"path":"#.to_string(),
        },
        &mut ctx,
    );
    // Input should remain the initial empty object while the JSON is incomplete.
    let tc1_input_partial = mode.task_doc.active_turn.as_ref().and_then(|t| {
        t.entries.iter().rev().find_map(|e| {
            if let crate::runtime::TurnEntry::ToolCall { id, input, .. } = e {
                if id == "tc1" {
                    return Some(input.clone());
                }
            }
            None
        })
    });
    assert_eq!(
        tc1_input_partial,
        Some(serde_json::Value::Object(Default::default())),
        "partial delta must not update pending tool call input"
    );

    // Second delta completes the JSON.
    mode.on_model_update(
        UiUpdate::StreamBlockDelta {
            index: 0,
            delta: r#""foo.rs"}"#.to_string(),
        },
        &mut ctx,
    );
    let tc1_input_complete = mode.task_doc.active_turn.as_ref().and_then(|t| {
        t.entries.iter().rev().find_map(|e| {
            if let crate::runtime::TurnEntry::ToolCall { id, input, .. } = e {
                if id == "tc1" {
                    return Some(input.clone());
                }
            }
            None
        })
    });
    assert_eq!(
        tc1_input_complete,
        Some(serde_json::json!({"path": "foo.rs"})),
        "complete delta must update pending tool call input with parsed value"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[detail] Input: path: foo.rs"),
        "complete delta must replace the pending transcript rows with the parsed preview"
    );
}

// -- scroll preservation -----------------------------------------------------

#[test]
fn test_tool_result_replacement_preserves_scroll_by_net_growth() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    // Prime the transcript with 18 pre-session notices so the viewport is
    // "full" before the turn starts (simulates the old history_state.lines).
    for index in 0..18 {
        mode.push_document_notice(
            format!("history row {index}"),
            crate::runtime::NoticeSeverity::Info,
        );
    }
    mode.begin_turn_capture("test".to_string());
    mode.transcript_scroll_offset = 4;

    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::ToolCall {
                id: "tc-scroll".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path":"src/main.rs"}),
                status: crate::state::ToolStatus::Executing,
            },
        },
        &mut ctx,
    );

    let previous_output_len = mode.expanded_output_row_count();
    let previous_scroll_offset = mode.transcript_scroll_offset;

    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolResult {
                tool_call_id: "tc-scroll".to_string(),
                output: "read_file completed\nline one\nline two\nline three\nline four"
                    .to_string(),
                is_error: false,
            },
        },
        &mut ctx,
    );

    let new_output_len = mode.expanded_output_row_count();
    assert!(
        new_output_len > previous_output_len,
        "completed tool paragraph should grow compared with the pending preview"
    );
    assert_eq!(
        mode.transcript_scroll_offset,
        previous_scroll_offset + (new_output_len - previous_output_len),
        "scroll preservation must use the net replacement growth rather than the full completed paragraph height"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[tool] read_file · src/main.rs · Response complete."),
        "completed tool paragraph must replace the pending transcript rows"
    );
}

// -- duplicate tool-call folding ---------------------------------------------

fn tool_call_start(index: usize, id: &str, name: &str, input: serde_json::Value) -> UiUpdate {
    UiUpdate::StreamBlockStart {
        index,
        block: StreamBlock::ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            input,
            status: crate::state::ToolStatus::Executing,
        },
    }
}

fn tool_result(index: usize, tool_call_id: &str, output: &str) -> UiUpdate {
    UiUpdate::StreamBlockStart {
        index,
        block: StreamBlock::ToolResult {
            tool_call_id: tool_call_id.to_string(),
            output: output.to_string(),
            is_error: false,
        },
    }
}

#[test]
fn test_duplicate_tool_calls_fold_to_repeated_indicator() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("task".to_string(), &mut ctx);

    let input = serde_json::json!({"path": "src/main.rs"});

    // First call
    mode.on_model_update(
        tool_call_start(0, "t1", "read_file", input.clone()),
        &mut ctx,
    );
    mode.on_model_update(tool_result(1, "t1", "fn main() {}"), &mut ctx);
    // Second identical call
    mode.on_model_update(
        tool_call_start(2, "t2", "read_file", input.clone()),
        &mut ctx,
    );
    mode.on_model_update(tool_result(3, "t2", "fn main() {}"), &mut ctx);

    let lines = &mode.history_lines();
    // The second identical completed tool call folds into a "(repeated ×N)" indicator;
    // only the first call renders a full [tool] header paragraph.
    let tool_headers: Vec<_> = lines
        .iter()
        .filter(|l| l.starts_with("[tool] read_file"))
        .collect();
    assert_eq!(
        tool_headers.len(),
        1,
        "first duplicate renders normally; subsequent identical calls fold to indicator; got:\n{:#?}",
        lines
    );
    assert!(
        lines.iter().any(|l| l.starts_with("[detail] (repeated")),
        "expected a fold indicator row for the duplicate call; got:\n{:#?}",
        lines
    );
}

#[test]
fn test_different_consecutive_tool_calls_not_folded() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("task".to_string(), &mut ctx);

    // First call: read_file
    mode.on_model_update(
        tool_call_start(0, "t1", "read_file", serde_json::json!({"path": "a.rs"})),
        &mut ctx,
    );
    mode.on_model_update(tool_result(1, "t1", "content of a"), &mut ctx);

    // Second call: different tool
    mode.on_model_update(
        tool_call_start(
            2,
            "t2",
            "write_file",
            serde_json::json!({"path": "b.rs", "content": "x"}),
        ),
        &mut ctx,
    );
    mode.on_model_update(tool_result(3, "t2", "written"), &mut ctx);

    let lines = &mode.history_lines();
    assert!(
        !lines.iter().any(|l| l.starts_with("[detail] (repeated")),
        "different tool calls must not produce a folded-duplicate line; got:\n{:#?}",
        lines
    );
    // Each unique tool result must produce its own completed [tool] header row.
    // Pending rows also emit [tool] headers, so we expect 4 total (2 pending + 2 completed).
    let tool_headers: Vec<_> = lines.iter().filter(|l| l.starts_with("[tool]")).collect();
    assert!(
        tool_headers.len() >= 2,
        "expected at least two [tool] header lines for different tool calls; got:\n{:#?}",
        lines
    );
}

#[test]
fn test_tool_fold_state_resets_after_turn_ends() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    // Turn 1: complete a tool call.
    mode.on_user_input("first".to_string(), &mut ctx);
    let input = serde_json::json!({"path": "src/main.rs"});
    mode.on_model_update(
        tool_call_start(0, "t1", "read_file", input.clone()),
        &mut ctx,
    );
    mode.on_model_update(tool_result(1, "t1", "content"), &mut ctx);
    // NOTE: duplicate_tool_count / last_completed_tool_header were removed in
    // the document-projector refactor.  Fold state is verified via history_lines().

    // End the turn via Error (causes reset_turn_capture).
    mode.on_model_update(UiUpdate::Error("test reset".to_string()), &mut ctx);

    // Turn 2: same tool call in a fresh turn must not fold.
    mode.on_user_input("second".to_string(), &mut ctx);
    mode.on_model_update(
        tool_call_start(0, "t2", "read_file", input.clone()),
        &mut ctx,
    );
    mode.on_model_update(tool_result(1, "t2", "content"), &mut ctx);

    let lines = &mode.history_lines();
    assert!(
        !lines.iter().any(|l| l.starts_with("[detail] (repeated")),
        "tool call in new turn must not fold against previous turn; got:\n{:#?}",
        lines
    );
}

#[test]
fn test_same_name_different_target_tool_calls_fold_into_paragraph() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("list project".to_string(), &mut ctx);

    // First list_files call for path "src"
    mode.on_model_update(
        tool_call_start(0, "t1", "list_files", serde_json::json!({"path": "src"})),
        &mut ctx,
    );
    mode.on_model_update(tool_result(1, "t1", "src/main.rs\nsrc/lib.rs"), &mut ctx);

    // Second list_files call for a different path "tests"
    mode.on_model_update(
        tool_call_start(2, "t2", "list_files", serde_json::json!({"path": "tests"})),
        &mut ctx,
    );
    mode.on_model_update(tool_result(3, "t2", "tests/integration.rs"), &mut ctx);

    let lines = &mode.history_lines();

    // The second completed call must NOT produce its own [tool] header —
    // it should be folded into the paragraph started by the first call.
    // Expect exactly 2 completed [tool] headers: 1 pending for t1 (before
    // its result), 1 completed for t1, and t2's pending and completed
    // headers are both folded. The pending header for t1 is emitted before
    // t2 exists, so 2 pending + 1 completed = 3 max.
    // NOTE: The document-projector refactor removed same-name-tool folding.
    // Each tool call now renders its own paragraph.
    let tool_headers: Vec<_> = lines
        .iter()
        .filter(|l| l.starts_with("[tool] list_files"))
        .collect();
    assert_eq!(
        tool_headers.len(),
        2,
        "both same-name tool calls must render individually; got:\n{:#?}",
        lines
    );

    // Both results must have their evidence rows in the transcript.
    assert!(
        lines.iter().any(|l| l.contains("src/main.rs")),
        "first tool result evidence must be preserved; got:\n{:#?}",
        lines
    );
    assert!(
        lines.iter().any(|l| l.contains("tests/integration.rs")),
        "second tool result evidence must be preserved after folding; got:\n{:#?}",
        lines
    );

    // NOTE: same_name_tool_count was removed in the document-projector refactor;
    // folding behaviour is verified via the history_lines() assertions above.
}

// -- transcript rendering ----------------------------------------------------

#[test]
fn test_cross_round_duplicate_tool_calls_fold_across_assistant_blocks() {
    // Regression test: the model sends an empty or whitespace-only
    // AssistantBlock(FinalText) between each tool-call round.  The dedup
    // tracker must navigate those empty blocks so that identical consecutive
    // tool calls across rounds are folded.
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("read release workflow".to_string(), &mut ctx);

    let input = serde_json::json!({"path": ".github/workflows/release.yml"});

    // Round 1: assistant says "I'll read..." → tool call → tool result
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::FinalText {
                content: String::new(),
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        tool_call_start(1, "t1", "read_file", input.clone()),
        &mut ctx,
    );
    mode.on_model_update(tool_result(2, "t1", "name: release"), &mut ctx);

    // Round 2: empty assistant block → identical tool call → same result
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 3,
            block: StreamBlock::FinalText {
                content: String::new(),
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        tool_call_start(4, "t2", "read_file", input.clone()),
        &mut ctx,
    );
    mode.on_model_update(tool_result(5, "t2", "name: release"), &mut ctx);

    // Round 3: another empty assistant block → third identical call
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 6,
            block: StreamBlock::FinalText {
                content: String::new(),
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        tool_call_start(7, "t3", "read_file", input.clone()),
        &mut ctx,
    );
    mode.on_model_update(tool_result(8, "t3", "name: release"), &mut ctx);

    let lines = &mode.history_lines();
    let tool_headers: Vec<_> = lines
        .iter()
        .filter(|l| l.starts_with("[tool] read_file"))
        .collect();
    assert_eq!(
        tool_headers.len(),
        1,
        "cross-round duplicates separated by empty assistant blocks must fold; got:\n{:#?}",
        lines
    );
    assert!(
        lines.iter().any(|l| l.contains("repeated \u{d7}3")),
        "expected fold indicator showing 3 identical calls; got:\n{:#?}",
        lines
    );
}

#[test]
fn test_substantive_assistant_block_resets_dedup_tracker() {
    // When the model sends a non-empty assistant text block between tool
    // calls, the dedup tracker should reset so the next call renders fully.
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("examine files".to_string(), &mut ctx);

    let input = serde_json::json!({"path": "src/main.rs"});

    // Round 1
    mode.on_model_update(
        tool_call_start(0, "t1", "read_file", input.clone()),
        &mut ctx,
    );
    mode.on_model_update(tool_result(1, "t1", "fn main() {}"), &mut ctx);

    // Non-empty assistant text resets the tracker
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 2,
            block: StreamBlock::FinalText {
                content: "Let me check the same file again with different parameters.".to_string(),
            },
        },
        &mut ctx,
    );

    // Round 2: identical tool call, but preceded by substantive text
    mode.on_model_update(
        tool_call_start(3, "t2", "read_file", input.clone()),
        &mut ctx,
    );
    mode.on_model_update(tool_result(4, "t2", "fn main() {}"), &mut ctx);

    let lines = &mode.history_lines();
    let tool_headers: Vec<_> = lines
        .iter()
        .filter(|l| l.starts_with("[tool] read_file"))
        .collect();
    assert_eq!(
        tool_headers.len(),
        2,
        "substantive assistant text between identical tool calls must reset the tracker; got:\n{:#?}",
        lines
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("[detail] (repeated")),
        "no fold indicator when assistant text intervenes; got:\n{:#?}",
        lines
    );
}

#[test]
fn test_long_transcript_line_marks_omitted_characters() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("task".to_string(), &mut ctx);

    // Simulate a very long transcript line (e.g. large git diff output).
    let long_line = "x".repeat(1024);
    mode.on_model_update(UiUpdate::TranscriptLine(long_line.clone()), &mut ctx);

    let lines = &mode.history_lines();
    let clipped = lines
        .iter()
        .find(|l| l.contains("more chars omitted"))
        .expect("should have a clipped line with omitted-chars marker");
    assert!(
        clipped.len() < long_line.len(),
        "transcript lines exceeding 512 chars must be shortened; got {} chars",
        clipped.len()
    );
}

#[test]
fn test_edit_loop_lines_rendered_as_transcript() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("edit task".to_string(), &mut ctx);

    // Edit loop turn markers must be stored in history.
    mode.on_model_update(
        UiUpdate::TranscriptLine("[edit loop turn 1/6]".to_string()),
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::TranscriptLine("[edit loop: running validation]".to_string()),
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::TranscriptLine("[edit loop: validation passed]".to_string()),
        &mut ctx,
    );

    let lines = &mode.history_lines();
    assert!(
        lines.iter().any(|l| l.contains("edit loop turn 1/6")),
        "edit loop turn marker must appear in transcript history"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("edit loop: validation passed")),
        "edit loop validation status must appear in transcript history"
    );
}

#[test]
fn test_edit_loop_warning_preserved_in_transcript() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("edit task".to_string(), &mut ctx);

    mode.on_model_update(
        UiUpdate::TranscriptLine(
            "[edit loop warning: workspace has uncommitted changes; proceeding without mutating git state]"
                .to_string(),
        ),
        &mut ctx,
    );

    let lines = &mode.history_lines();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("edit loop warning: workspace has uncommitted changes")),
        "edit loop warning must appear in transcript history; got:\n{:#?}",
        lines
    );
}

#[test]
fn test_edit_loop_turn_error_preserved_in_transcript() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("edit task".to_string(), &mut ctx);

    mode.on_model_update(
        UiUpdate::TranscriptLine("[edit loop turn error: connection timeout]".to_string()),
        &mut ctx,
    );

    let lines = &mode.history_lines();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("edit loop turn error: connection timeout")),
        "edit loop turn error must appear in transcript history; got:\n{:#?}",
        lines
    );
}

#[test]
fn test_edit_loop_complete_emits_telemetry_line() {
    use crate::runtime::edit_loop::EditLoopOutcome;
    use crate::types::StreamTimings;
    use std::time::Instant;

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("edit task".to_string(), &mut ctx);

    // Simulate server metadata arriving during the edit loop turn.
    mode.turn_started_at = Some(Instant::now());
    mode.ttft = Some(std::time::Duration::from_millis(200));
    if let Some(active) = mode.task_doc.active_turn.as_mut() {
        active.timings = Some(StreamTimings {
            prompt_ms: Some(500.0),
            prompt_n: Some(100),
            predicted_ms: Some(1500.0),
            predicted_n: Some(50),
            ..Default::default()
        });
    }

    // EditLoopComplete should capture and emit telemetry.
    mode.on_model_update(
        UiUpdate::EditLoopComplete {
            outcome: EditLoopOutcome::Success {
                patch_applied: true,
                validate_passed: true,
            },
            last_validation_result: None,
        },
        &mut ctx,
    );

    let lines = &mode.history_lines();
    // Must contain the timing summary line.
    assert!(
        lines
            .iter()
            .any(|l| l.contains("total:") && l.contains("ttft:")),
        "EditLoopComplete must emit a telemetry summary line; got:\n{:#?}",
        lines
    );
}
