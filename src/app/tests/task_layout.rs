use super::*;

#[test]
fn test_task_layout_state_shows_waiting_output_without_prompt_duplication() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("check the build status".to_string(), &mut ctx);

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(state.output_rows.len(), 2);
    assert_eq!(
        state.output_rows[0],
        TranscriptRow::UserInput("check the build status".to_string())
    );

    assert!(
        matches!(&state.output_rows[1], TranscriptRow::WaitingPlaceholder(s) if s.starts_with("[thinking] Mapping adjacent sectors...")),
        "expected accepted ADR-039 waiting row, got: {:?}",
        state.output_rows[1]
    );
}

#[test]
fn test_task_layout_state_shows_server_read_progress_in_waiting_row() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("check the build status".to_string(), &mut ctx);
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
    assert!(
        state.output_rows[1]
            .as_display_str()
            .contains("\u{2191}:2048/2641")
    );
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
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 0,
            block: StreamBlock::Thinking {
                content: "trace branch\ncollect evidence".to_string(),
                collapsed: false,
            },
        },
        &mut ctx,
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
    assert_eq!(
        state.output_rows[0],
        TranscriptRow::UserInput("plan it".to_string())
    );
    assert!(
        state.output_rows.iter().any(|r| matches!(r, TranscriptRow::AssistantText { text, .. } if text == "streaming line\u{258c}")),
        "streaming content must appear in transcript"
    );
    assert_eq!(
        state.output_rows.last().expect("last row"),
        &TranscriptRow::AssistantText {
            text: "streaming line\u{258c}".to_string(),
            streaming: true
        }
    );
}

#[test]
fn test_task_layout_state_shows_approved_pending_tool_after_acceptance() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("plan it".to_string(), &mut ctx);
    if let Some(active) = mode.task_doc.active_pulse.as_mut() {
        active.entries.push(PulseEntry::ToolCall {
            step_id: 1,
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({"path":"src/main.rs"}),
            status: crate::state::ToolStatus::WaitingApproval,
        });
    }

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
        mode.task_doc.info.status,
        crate::runtime::TaskStatus::Running
    );
}

#[test]
fn test_task_layout_state_routes_streamed_response_to_output_pane() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("describe the project layout".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamDelta("the project has three modules".to_string()),
        &mut ctx,
    );

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(
        state.output_rows,
        vec![
            TranscriptRow::UserInput("describe the project layout".to_string()),
            TranscriptRow::AssistantText {
                text: "the project has three modules\u{258c}".to_string(),
                streaming: true
            },
        ]
    );
}

#[test]
fn test_task_layout_state_preserves_multiline_streamed_response_in_transcript() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("check the build status".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamDelta("first line\nsecond line".to_string()),
        &mut ctx,
    );

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(
        state.output_rows,
        vec![
            TranscriptRow::UserInput("check the build status".to_string()),
            TranscriptRow::AssistantText {
                text: "first line".to_string(),
                streaming: false
            },
            TranscriptRow::AssistantText {
                text: "second line\u{258c}".to_string(),
                streaming: true
            },
        ]
    );
}

#[test]
fn test_task_layout_state_preserves_structured_text_order_around_tool_rows() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("read file".to_string(), &mut ctx);
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
        UiUpdate::StreamDelta("I will read the file.".to_string()),
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolCall {
                id: "tool-1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path":"file.txt"}),
                status: crate::state::ToolStatus::Pending,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 2,
            block: StreamBlock::ToolResult {
                tool_call_id: "tool-1".to_string(),
                output: "file content retrieved".to_string(),
                is_error: false,
            },
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
    mode.on_model_update(
        UiUpdate::StreamDelta("The file contains project configuration.".to_string()),
        &mut ctx,
    );

    let state = mode.task_layout_state().expect("task layout state");
    let intro_idx = state
        .output_rows
        .iter()
        .position(|row| row.as_display_str().starts_with("I will read the file."))
        .expect("intro row");
    let tool_idx = state
        .output_rows
        .iter()
        .position(|row| {
            matches!(row, TranscriptRow::ToolHeader(s) if s.starts_with("read_file \u{00b7} ") && s.ends_with("Response complete."))
        })
        .expect("tool result row");
    let final_idx = state
        .output_rows
        .iter()
        .position(|row| {
            row.as_display_str()
                .starts_with("The file contains project configuration.")
        })
        .expect("final response row");

    assert!(
        intro_idx < tool_idx,
        "intro text must stay above the tool paragraph; got:\n{:#?}",
        state.output_rows
    );
    assert!(
        tool_idx < final_idx,
        "final text must stay below the tool paragraph; got:\n{:#?}",
        state.output_rows
    );
}

#[test]
fn test_commit_completed_turn_materializes_structured_stream_segments() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("read file".to_string(), &mut ctx);
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
        UiUpdate::StreamDelta("I will read the file.".to_string()),
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 1,
            block: StreamBlock::ToolCall {
                id: "tool-1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path":"file.txt"}),
                status: crate::state::ToolStatus::Pending,
            },
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamBlockStart {
            index: 2,
            block: StreamBlock::ToolResult {
                tool_call_id: "tool-1".to_string(),
                output: "file content retrieved".to_string(),
                is_error: false,
            },
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
    mode.on_model_update(
        UiUpdate::StreamDelta("The file contains project configuration.".to_string()),
        &mut ctx,
    );

    mode.commit_completed_turn(&ctx);

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(
        state.output_rows[0],
        TranscriptRow::UserInput("read file".to_string())
    );
    assert_eq!(
        state.output_rows[1],
        TranscriptRow::AssistantText {
            text: "I will read the file.".to_string(),
            streaming: false
        }
    );
    assert!(
        matches!(&state.output_rows[2], TranscriptRow::ToolHeader(s) if s.starts_with("read_file \u{00b7} ") && s.ends_with("Response complete."))
    );
    assert_eq!(
        state.output_rows.last(),
        Some(&TranscriptRow::AssistantText {
            text: "The file contains project configuration.".to_string(),
            streaming: false
        })
    );
}

#[test]
fn test_commit_completed_turn_uses_normalized_stream_text_once() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("summarize the changes".to_string(), &mut ctx);
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
        UiUpdate::StreamBlockDelta {
            index: 0,
            delta: "Hello".to_string(),
        },
        &mut ctx,
    );
    mode.on_model_update(UiUpdate::StreamDelta("Hello".to_string()), &mut ctx);

    mode.commit_completed_turn(&ctx);

    let last_pulse = mode
        .task_doc
        .completed_turns
        .last()
        .expect("recorded pulse");
    let response =
        crate::app::transcript_projection::extract_assistant_response(&last_pulse.entries);
    assert_eq!(response, "Hello");

    let state = mode.task_layout_state().expect("task layout state");
    let joined = state
        .output_rows
        .iter()
        .map(|r| r.to_history_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        joined.matches("Hello").count(),
        1,
        "committed transcript must keep normalized stream text once: {joined:?}"
    );
}

#[test]
fn test_task_layout_state_keeps_prior_responses_visible_after_turn_completion() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("inspect the file".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("Done.".to_string()), &mut ctx);
    mode.commit_completed_turn(&ctx);

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(
        state.output_rows,
        vec![
            TranscriptRow::UserInput("inspect the file".to_string()),
            TranscriptRow::AssistantText {
                text: "Done.".to_string(),
                streaming: false
            },
        ]
    );
}

#[test]
fn test_manual_timeline_selection_opens_tool_inspector() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("inspect the file".to_string(), &mut ctx);
    if let Some(active) = mode.task_doc.active_pulse.as_mut() {
        active.entries.push(PulseEntry::ToolCall {
            step_id: 1,
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({}),
            status: crate::state::ToolStatus::Complete,
        });
    }
    mode.timeline_follow_mode = false;
    mode.selected_timeline_index = 1;

    let state = mode.task_layout_state().expect("task layout state");
    assert!(state.output_title.starts_with("Inspector"));
    assert!(state.output_title.contains("read_file"));
    assert_eq!(state.output_rows[0].as_display_str(), "Tool: read_file");
}

#[test]
fn test_timeline_home_and_end_via_frontend_event_preserve_browse_contract() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("inspect the file".to_string(), &mut ctx);
    if let Some(active) = mode.task_doc.active_pulse.as_mut() {
        active.entries.push(PulseEntry::ToolCall {
            step_id: 1,
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({}),
            status: crate::state::ToolStatus::Complete,
        });
        active.entries.push(PulseEntry::ToolCall {
            step_id: 2,
            id: "tc2".to_string(),
            name: "check".to_string(),
            input: serde_json::json!({}),
            status: crate::state::ToolStatus::Complete,
        });
    }
    mode.timeline_follow_mode = false;
    mode.selected_timeline_index = 2;

    mode.on_frontend_event(
        crate::runtime::frontend::UserInputEvent::Scroll {
            target: crate::runtime::frontend::ScrollTarget::Timeline,
            action: crate::runtime::frontend::ScrollAction::Home,
        },
        &mut ctx,
    );
    assert_eq!(mode.selected_timeline_index, 0);
    assert!(!mode.timeline_follow_mode);

    mode.on_frontend_event(
        crate::runtime::frontend::UserInputEvent::Scroll {
            target: crate::runtime::frontend::ScrollTarget::Timeline,
            action: crate::runtime::frontend::ScrollAction::End,
        },
        &mut ctx,
    );
    assert_eq!(mode.selected_timeline_index, 2);
    assert!(mode.timeline_follow_mode);
}

#[test]
fn test_manual_timeline_browse_stays_selected_when_new_transcript_rows_arrive() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("inspect the file".to_string(), &mut ctx);
    if let Some(active) = mode.task_doc.active_pulse.as_mut() {
        active.entries.push(PulseEntry::ToolCall {
            step_id: 1,
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({}),
            status: crate::state::ToolStatus::Complete,
        });
    }
    mode.timeline_follow_mode = false;
    mode.selected_timeline_index = 1;

    mode.on_model_update(
        UiUpdate::TranscriptLine("later detail".to_string()),
        &mut ctx,
    );

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(state.selected_step, 1);
    assert!(!state.follow_mode);
    assert_eq!(state.output_rows[0].as_display_str(), "Tool: read_file");
}

#[test]
fn test_task_layout_state_shows_pending_tool_call_in_timeline() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("ship the fix".to_string(), &mut ctx);
    if let Some(active) = mode.task_doc.active_pulse.as_mut() {
        for (i, name) in [
            "read_file",
            "edit_file",
            "run_command",
            "write_file",
            "apply_patch",
        ]
        .iter()
        .enumerate()
        {
            active.entries.push(PulseEntry::ToolCall {
                step_id: (i + 1) as u64,
                id: format!("tc{i}"),
                name: name.to_string(),
                input: serde_json::json!({}),
                status: crate::state::ToolStatus::Complete,
            });
        }
        active.entries.push(PulseEntry::ToolCall {
            step_id: 6,
            id: "tc6".to_string(),
            name: "validate".to_string(),
            input: serde_json::json!({}),
            status: crate::state::ToolStatus::Pending,
        });
    }

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
    if let Some(active) = mode.task_doc.active_pulse.as_mut() {
        active.entries.push(PulseEntry::ToolCall {
            step_id: 3,
            id: "tc3".to_string(),
            name: "edit_file".to_string(),
            input: serde_json::json!({}),
            status: crate::state::ToolStatus::Pending,
        });
        active.entries.push(PulseEntry::ToolCall {
            step_id: 4,
            id: "tc4".to_string(),
            name: "validate".to_string(),
            input: serde_json::json!({}),
            status: crate::state::ToolStatus::Pending,
        });
    }

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
    if let Some(active) = mode.task_doc.active_pulse.as_mut() {
        active.entries.push(PulseEntry::ToolCall {
            step_id: 1,
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({}),
            status: crate::state::ToolStatus::Complete,
        });
        active.entries.push(PulseEntry::ToolCall {
            step_id: 2,
            id: "tc2".to_string(),
            name: "run_command".to_string(),
            input: serde_json::json!({}),
            status: crate::state::ToolStatus::Pending,
        });
    }
    if let Some(active) = mode.task_doc.active_pulse.as_mut() {
        active.command_sessions.insert(
            99,
            crate::runtime::task_document::CommandSessionDocument {
                session_id: 99,
                command: "cargo nextest run".to_string(),
                pid: Some(4242),
                status: "running".to_string(),
                output_tail: vec![],
            },
        );
    }

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
            "read_file: done".to_string(),
            "run_command: Mapping adjacent sectors...".to_string(),
            "cargo nextest run: Mapping adjacent sectors...".to_string(),
        ]
    );
    assert_eq!(mode.timeline_entry_count(), 4);
}

#[test]
fn test_timeline_end_reaches_last_mixed_step_with_command_sessions() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("run the validation".to_string(), &mut ctx);
    if let Some(active) = mode.task_doc.active_pulse.as_mut() {
        active.entries.push(PulseEntry::ToolCall {
            step_id: 1,
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({}),
            status: crate::state::ToolStatus::Complete,
        });
        active.entries.push(PulseEntry::ToolCall {
            step_id: 2,
            id: "tc2".to_string(),
            name: "run_command".to_string(),
            input: serde_json::json!({}),
            status: crate::state::ToolStatus::Pending,
        });
        active.command_sessions.insert(
            99,
            crate::runtime::task_document::CommandSessionDocument {
                session_id: 99,
                command: "cargo nextest run".to_string(),
                pid: Some(4242),
                status: "running".to_string(),
                output_tail: vec![],
            },
        );
    }

    mode.timeline_follow_mode = false;
    mode.selected_timeline_index = 0;

    mode.on_frontend_event(
        crate::runtime::frontend::UserInputEvent::Scroll {
            target: crate::runtime::frontend::ScrollTarget::Timeline,
            action: crate::runtime::frontend::ScrollAction::End,
        },
        &mut ctx,
    );

    assert_eq!(mode.selected_timeline_index, 3);
    assert!(mode.timeline_follow_mode);
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
fn test_follow_mode_auto_advances_selected_step_when_new_entries_arrive() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("run lint".to_string(), &mut ctx);
    if let Some(active) = mode.task_doc.active_pulse.as_mut() {
        active.entries.push(PulseEntry::ToolCall {
            step_id: 1,
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({}),
            status: crate::state::ToolStatus::Complete,
        });
    }

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(state.total_steps, 2);

    assert!(state.follow_mode);
    assert_eq!(state.selected_step, 1);

    if let Some(active) = mode.task_doc.active_pulse.as_mut() {
        active.entries.push(PulseEntry::ToolCall {
            step_id: 2,
            id: "tc2".to_string(),
            name: "run_command".to_string(),
            input: serde_json::json!({}),
            status: crate::state::ToolStatus::Pending,
        });
    }

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(state.total_steps, 3);
    assert!(
        state.follow_mode,
        "follow mode must remain active across new entries"
    );
    assert_eq!(
        state.selected_step, 2,
        "selected_step must auto-advance to the latest entry in follow mode"
    );
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
    if let Some(active) = mode.task_doc.active_pulse.as_mut() {
        active.timings = Some(crate::types::StreamTimings {
            prompt_ms: Some(1000.0),
            prompt_n: Some(10),
            predicted_ms: Some(500.0),
            predicted_n: Some(5),
            ..Default::default()
        });
    }

    mode.on_model_update(UiUpdate::PulseComplete, &mut ctx);

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
            .any(|line| line.as_display_str().starts_with("[\u{2191}:1.0s")),
        "the transcript should continue to carry the inline timing line"
    );
}

#[test]
fn test_task_layout_state_merges_live_changed_files_before_turn_commit() {
    let mut mode = TuiMode::new();
    use crate::runtime::task_document::{PulseOutcome, TurnDocument};
    use crate::usage::PulseTokens;

    mode.task_doc.completed_turns.push(TurnDocument {
        turn_index: 0,
        input: "prev".to_string(),
        entries: vec![],
        outcome: PulseOutcome::Completed,
        changed_files: vec!["src/lib.rs".to_string()],
        command_history: vec![],
        tokens: PulseTokens {
            input: 0,
            output: 0,
            ..Default::default()
        },
        started_at_ms: 0,
        completed_at_ms: 1,
        ttft_ms: None,
        timings: None,
    });
    mode.begin_turn_capture("curr".to_string());
    if let Some(active) = mode.task_doc.active_pulse.as_mut() {
        active.changed_files.insert("src/main.rs".to_string());
    }

    let state = mode.task_layout_state().expect("task layout state");

    assert_eq!(
        state.changed_files,
        vec!["src/lib.rs".to_string(), "src/main.rs".to_string()],
        "task layout should expose persisted and in-flight changed files together"
    );
}

#[test]
fn test_task_layout_state_exposes_structured_footer_inputs() {
    let mut mode = TuiMode::new();

    mode.last_assembled_context = Some(crate::runtime::AssembledContext {
        file_rollups: vec![crate::runtime::FileRollup {
            path: std::path::PathBuf::from("src/main.rs"),
            content: Some("fn main() {}\n".to_string()),
            content_limited: false,
        }],
        git_status_summary: Some(" M src/main.rs".to_string()),
        recent_diff: None,
        has_staged_changes: false,
        has_working_tree_changes: false,
        git_dir: None,
        committer_name: None,
        staged_paths: vec![],
        related_paths: vec![std::path::PathBuf::from("src/lib.rs")],
        cache_hits: 2,
        cache_misses: 1,
    });

    let state = mode.task_layout_state().expect("task layout state");
    let context = state.telemetry.context_summary.expect("context summary");

    assert_eq!(state.telemetry.model_name, "local/default");
    assert_eq!(
        state.telemetry.model_backend,
        Some(crate::runtime::ModelBackendKind::LocalRuntime)
    );
    assert_eq!(
        state.telemetry.sandbox_kind,
        Some(crate::runtime::SandboxKind::Passthrough)
    );
    assert_eq!(context.file_rollups, 1);
    assert_eq!(context.related_paths, 1);
    assert_eq!(context.cache_hits, 2);
    assert_eq!(context.cache_misses, 1);
    assert!(context.git_context_included);
}

#[test]
fn test_inspector_title_includes_row_count() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("multilang check".to_string(), &mut ctx);
    if let Some(active) = mode.task_doc.active_pulse.as_mut() {
        active.entries.push(PulseEntry::ToolCall {
            step_id: 1,
            id: "tc1".to_string(),
            name: "run_command".to_string(),
            input: serde_json::json!({}),
            status: crate::state::ToolStatus::Complete,
        });
    }
    mode.timeline_follow_mode = false;
    mode.selected_timeline_index = 1;

    let state = mode.task_layout_state().expect("task layout state");

    assert!(
        state.output_title.contains("rows"),
        "inspector title should contain row count: {:?}",
        state.output_title
    );
    assert!(
        state.output_title.starts_with("Inspector"),
        "inspector title should start with Inspector: {:?}",
        state.output_title
    );
}

#[test]
fn test_final_text_block_delta_does_not_duplicate_normalized_stream_rows() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("summarize the changes".to_string(), &mut ctx);

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
        UiUpdate::StreamBlockDelta {
            index: 0,
            delta: "Hello ".to_string(),
        },
        &mut ctx,
    );
    mode.on_model_update(UiUpdate::StreamDelta("Hello ".to_string()), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamBlockDelta {
            index: 0,
            delta: "world!".to_string(),
        },
        &mut ctx,
    );
    mode.on_model_update(UiUpdate::StreamDelta("world!".to_string()), &mut ctx);

    let state = mode.task_layout_state().expect("task layout state");
    let joined = state
        .output_rows
        .iter()
        .map(|r| r.to_history_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("Hello world!"),
        "normalized stream text must appear in output rows: {joined:?}"
    );
    assert_eq!(
        joined.matches("Hello world!").count(),
        1,
        "textual block metadata must not duplicate normalized stream text: {joined:?}"
    );
}

#[test]
fn test_thinking_block_delta_does_not_duplicate_normalized_stream_rows() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("think deeply".to_string(), &mut ctx);

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
            delta: "analyzing the problem".to_string(),
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::StreamDelta("analyzing the problem".to_string()),
        &mut ctx,
    );

    let state = mode.task_layout_state().expect("task layout state");
    let joined = state
        .output_rows
        .iter()
        .map(|r| r.to_history_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("analyzing the problem"),
        "normalized thinking text must appear in output rows: {joined:?}"
    );
    assert_eq!(
        joined.matches("analyzing the problem").count(),
        1,
        "textual thinking metadata must not duplicate normalized stream text: {joined:?}"
    );
}

#[test]
fn task_layout_keeps_active_rows_in_the_owned_viewport() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("stream the answer".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("working".to_string()), &mut ctx);

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(
        state.output_rows,
        vec![
            TranscriptRow::UserInput("stream the answer".to_string()),
            TranscriptRow::AssistantText {
                text: "working\u{258c}".to_string(),
                streaming: true,
            },
        ]
    );
}

#[test]
fn task_layout_keeps_completed_turns_in_the_owned_viewport() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("finish the answer".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("done".to_string()), &mut ctx);
    mode.commit_completed_turn(&ctx);

    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(
        state.output_rows,
        vec![
            TranscriptRow::UserInput("finish the answer".to_string()),
            TranscriptRow::AssistantText {
                text: "done".to_string(),
                streaming: false,
            },
        ]
    );
}
