use super::*;

#[test]
fn test_task_layout_state_shows_waiting_output_without_prompt_duplication() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("hi".to_string(), &mut ctx);

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(state.output_rows.len(), 2);
    assert_eq!(state.output_rows[0], "> hi");
    // The second row is the ADR-039 canonical waiting phrase with elapsed suffix.
    assert!(
        state.output_rows[1].starts_with("[thinking] Mapping adjacent sectors..."),
        "expected canonical ADR-039 waiting row, got: {:?}",
        state.output_rows[1]
    );
}

#[test]
fn test_task_layout_state_shows_server_read_progress_in_waiting_row() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("hi".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::ServerMetadata(Box::new(crate::types::StreamChunkMetadata {
            prompt_progress: Some(crate::types::StreamPromptProgress {
                total: Some(2641),
                processed: Some(2048),
                cache: Some(0),
                time_ms: Some(153341.0),
            }),
            ..Default::default()
        })),
        &mut ctx,
    );

    let state = mode.task_layout_state().expect("task layout state");
    assert!(state.output_rows[1].contains("\u{2191}:2048/2641"));
    assert_eq!(state.telemetry.mode, "streaming");
    assert_eq!(state.telemetry.approval, "none");
    assert!(
        state
            .telemetry
            .waiting_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("\u{2191}:2048/2641")),
        "structured telemetry must preserve prompt-read progress"
    );
}

#[test]
fn test_task_layout_state_transcript_streaming_with_pending_approval() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("plan it".to_string(), &mut ctx);
    mode.active_stream_blocks.insert(
        0,
        StreamBlock::Thinking {
            content: "trace branch\ncollect evidence".to_string(),
            collapsed: false,
        },
    );
    mode.pending_turn_tool_calls.insert(
        "tool-1".to_string(),
        PendingTurnToolCall {
            step_id: 1,
            name: "read_file".to_string(),
            input_preview: "{\"path\":\"src/main.rs\"}".to_string(),
            input: serde_json::json!({"path":"src/main.rs"}),
        },
    );
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.overlay_state.pending_approval = Some(PendingApproval {
        step_id: Some(1),
        tool_name: "read_file".to_string(),
        input_preview: "{\"path\":\"src/main.rs\"}".to_string(),
        action: PendingApprovalAction::Tool(response_tx),
    });
    mode.on_model_update(
        UiUpdate::StreamDelta("streaming line".to_string()),
        &mut ctx,
    );

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(state.output_rows[0], "> plan it");
    assert_eq!(state.output_rows[1], "streaming line▌");
    assert_eq!(
        state.output_rows.last().expect("last row"),
        "streaming line▌"
    );
}

#[test]
fn test_task_layout_state_shows_approved_pending_tool_after_acceptance() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("plan it".to_string(), &mut ctx);
    mode.pending_turn_tool_calls.insert(
        "tool-1".to_string(),
        PendingTurnToolCall {
            step_id: 1,
            name: "read_file".to_string(),
            input_preview: "{\"path\":\"src/main.rs\"}".to_string(),
            input: serde_json::json!({"path":"src/main.rs"}),
        },
    );

    let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "{\"path\":\"src/main.rs\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    let awaiting = mode
        .task_layout_state()
        .expect("task layout state")
        .timeline_entries
        .into_iter()
        .find(|entry| entry.step_id == 1)
        .expect("pending entry");
    assert_eq!(awaiting.lifecycle, StepLifecycle::AwaitingApproval);

    mode.resolve_pending_approval(true, &ctx);

    let approved = mode
        .task_layout_state()
        .expect("task layout state")
        .timeline_entries
        .into_iter()
        .find(|entry| entry.step_id == 1)
        .expect("approved entry");
    assert_eq!(approved.lifecycle, StepLifecycle::Approved);
    assert_eq!(approved.label, "read_file: approved");
    assert_eq!(
        mode.current_task.status,
        crate::runtime::TaskStatus::Running
    );
}

#[test]
fn test_task_layout_state_routes_streamed_response_to_output_pane() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("hi".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamDelta("hello from model".to_string()),
        &mut ctx,
    );

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(
        state.output_rows,
        vec!["> hi".to_string(), "hello from model▌".to_string()]
    );
}

#[test]
fn test_task_layout_state_preserves_multiline_streamed_response_in_transcript() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("hi".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamDelta("first line\nsecond line".to_string()),
        &mut ctx,
    );

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(
        state.output_rows,
        vec![
            "> hi".to_string(),
            "first line".to_string(),
            "second line▌".to_string(),
        ]
    );
}

#[test]
fn test_task_layout_state_keeps_prior_responses_visible_after_turn_completion() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("inspect the file".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("Done.".to_string()), &mut ctx);
    mode.current_turn_tool_invocations = vec![ToolInvocationSummary {
        step_id: 1,
        name: "read_file".to_string(),
        outcome: "42 lines read from src/main.rs\nfn main() {}".to_string(),
    }];
    mode.commit_completed_turn(&ctx);
    mode.history_state.turn_in_progress = false;

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(
        state.output_rows,
        vec!["> inspect the file".to_string(), "Done.".to_string()]
    );
}

#[test]
fn test_manual_timeline_selection_opens_tool_inspector() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("inspect the file".to_string(), &mut ctx);
    mode.current_turn_tool_invocations = vec![ToolInvocationSummary {
        step_id: 1,
        name: "read_file".to_string(),
        outcome: "42 lines read from src/main.rs".to_string(),
    }];
    mode.timeline_follow_mode = false;
    mode.selected_timeline_index = 1;

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(state.output_title, "Inspector");
    assert_eq!(state.output_rows[0], "Tool: read_file");
    assert_eq!(
        state.output_rows[1],
        "Outcome: 42 lines read from src/main.rs"
    );
}

#[test]
fn test_task_layout_state_shows_pending_tool_call_in_timeline() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("ship the fix".to_string(), &mut ctx);
    mode.current_turn_tool_invocations = vec![
        ToolInvocationSummary {
            step_id: 1,
            name: "read_file".to_string(),
            outcome: "ok".to_string(),
        },
        ToolInvocationSummary {
            step_id: 2,
            name: "edit_file".to_string(),
            outcome: "ok".to_string(),
        },
        ToolInvocationSummary {
            step_id: 3,
            name: "run_command".to_string(),
            outcome: "ok".to_string(),
        },
        ToolInvocationSummary {
            step_id: 4,
            name: "write_file".to_string(),
            outcome: "ok".to_string(),
        },
        ToolInvocationSummary {
            step_id: 5,
            name: "apply_patch".to_string(),
            outcome: "ok".to_string(),
        },
    ];
    mode.pending_turn_tool_calls.insert(
        "tool-1".to_string(),
        PendingTurnToolCall {
            step_id: 6,
            name: "validate".to_string(),
            input_preview: "{}".to_string(),
            input: serde_json::json!({}),
        },
    );

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(
        state.timeline_entries.len(),
        7,
        "timeline should contain user input + 5 completed + 1 pending"
    );
}

#[test]
fn test_task_layout_state_sorts_pending_tool_calls_by_step_id() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("ship the fix".to_string(), &mut ctx);
    mode.pending_turn_tool_calls.insert(
        "z-tool".to_string(),
        PendingTurnToolCall {
            step_id: 4,
            name: "validate".to_string(),
            input_preview: "{}".to_string(),
            input: serde_json::json!({}),
        },
    );
    mode.pending_turn_tool_calls.insert(
        "a-tool".to_string(),
        PendingTurnToolCall {
            step_id: 3,
            name: "edit_file".to_string(),
            input_preview: "{}".to_string(),
            input: serde_json::json!({}),
        },
    );

    let state = mode.task_layout_state().expect("task layout state");
    let labels = state
        .timeline_entries
        .iter()
        .skip(1)
        .map(|entry| entry.label.clone())
        .collect::<Vec<_>>();

    assert_eq!(
        labels,
        vec![
            "edit_file: Mapping adjacent sectors...",
            "validate: Mapping adjacent sectors..."
        ]
    );
}

#[test]
fn test_task_layout_state_keeps_command_sessions_alongside_other_steps() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("run the validation".to_string(), &mut ctx);
    mode.current_turn_tool_invocations = vec![ToolInvocationSummary {
        step_id: 1,
        name: "read_file".to_string(),
        outcome: "ok".to_string(),
    }];
    mode.pending_turn_tool_calls.insert(
        "tool-1".to_string(),
        PendingTurnToolCall {
            step_id: 2,
            name: "run_command".to_string(),
            input_preview: "{}".to_string(),
            input: serde_json::json!({}),
        },
    );
    mode.command_sessions.push(CommandSessionState {
        id: 99,
        command: "cargo nextest run -j 2".to_string(),
        pid: Some(4242),
        status: "running".to_string(),
    });

    let state = mode.task_layout_state().expect("task layout state");
    let labels = state
        .timeline_entries
        .iter()
        .map(|entry| entry.label.clone())
        .collect::<Vec<_>>();

    assert_eq!(
        labels,
        vec![
            "run the validation".to_string(),
            "read_file · Response complete.".to_string(),
            "run_command: Mapping adjacent sectors...".to_string(),
            "cargo nextest run -j 2: Mapping adjacent sectors...".to_string(),
        ]
    );
}

#[test]
fn test_task_layout_state_clamps_selected_step_when_timeline_is_empty() {
    let mut mode = TuiMode::new();
    mode.selected_timeline_index = 42;
    mode.timeline_follow_mode = false;

    let state = mode.task_layout_state().expect("task layout state");

    assert!(state.timeline_entries.is_empty());
    assert_eq!(state.total_steps, 0);
    assert_eq!(state.selected_step, 0);
}

#[test]
fn test_timeline_down_disables_follow_mode_until_end() {
    let mut mode = TuiMode::new();
    mode.timeline_follow_mode = true;
    mode.selected_timeline_index = 1;

    mode.apply_timeline_down(5);

    assert_eq!(mode.selected_timeline_index, 2);
    assert!(!mode.timeline_follow_mode);

    mode.apply_timeline_end(5);
    assert!(mode.timeline_follow_mode);
}

#[test]
fn test_timeline_page_down_disables_follow_mode_until_end() {
    let mut mode = TuiMode::new();
    mode.timeline_follow_mode = true;
    mode.selected_timeline_index = 1;

    mode.apply_timeline_scroll_action(ScrollAction::PageDown(2), 10);

    assert_eq!(mode.selected_timeline_index, 3);
    assert!(!mode.timeline_follow_mode);

    mode.apply_timeline_scroll_action(ScrollAction::PageDown(10), 10);
    assert!(mode.timeline_follow_mode);
}

#[test]
fn test_output_scroll_commands_use_bottom_anchored_prompt_surface() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("summarise the diff".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::TranscriptLine("line-1".to_string()), &mut ctx);
    mode.on_model_update(UiUpdate::TranscriptLine("line-2".to_string()), &mut ctx);
    mode.on_model_update(UiUpdate::TranscriptLine("line-3".to_string()), &mut ctx);

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::Output,
            action: ScrollAction::PageUp(2),
        },
        &mut ctx,
    );
    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(state.output_scroll_anchor, OutputScrollAnchor::Bottom);
    assert_eq!(state.output_scroll_offset, 2);

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::Output,
            action: ScrollAction::LineDown,
        },
        &mut ctx,
    );
    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(state.output_scroll_offset, 1);

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::Output,
            action: ScrollAction::End,
        },
        &mut ctx,
    );
    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(state.output_scroll_offset, 0);
}

#[test]
fn test_task_layout_state_exposes_turn_timing_summary_in_structured_telemetry() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("summarise the diff".to_string(), &mut ctx);
    mode.turn_started_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(3));
    mode.current_turn_timings = Some(crate::types::StreamTimings {
        prompt_ms: Some(1000.0),
        prompt_n: Some(10),
        predicted_ms: Some(500.0),
        predicted_n: Some(5),
        ..Default::default()
    });

    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

    let state = mode.task_layout_state().expect("task layout state");
    let summary = state
        .telemetry
        .timing_summary
        .as_deref()
        .expect("timing summary");
    assert!(
        summary.contains("\u{2191}:1.0s (10 tok)") && summary.contains("\u{2193}:0.5s (5 tok)"),
        "structured telemetry must expose the compact timing summary, got: {summary}"
    );
    assert!(
        state
            .output_rows
            .iter()
            .any(|line| line.starts_with("[\u{2191}:1.0s")),
        "the transcript should continue to carry the inline timing line"
    );
}
