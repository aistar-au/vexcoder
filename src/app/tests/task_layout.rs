use super::*;

#[test]
fn user_input_produces_waiting_placeholder_row() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("check the build status".to_string(), &mut ctx);
    let state = mode.task_layout_state().expect("task layout state");
    assert_eq!(
        state.output_rows[0],
        TranscriptRow::UserInput("check the build status".to_string())
    );
    assert!(
        matches!(&state.output_rows[1], TranscriptRow::WaitingPlaceholder(s) if s.starts_with("[thinking]")),
        "expected waiting row, got: {:?}",
        state.output_rows[1]
    );
}

#[test]
fn stream_delta_appears_in_transcript_output_rows() {
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
                text: "the project has three modules|".to_string(),
                streaming: true
            },
        ]
    );
}

#[test]
fn tool_approval_lifecycle_transitions_from_pending_to_approved() {
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
    let (response_tx, _rx) = tokio::sync::oneshot::channel::<bool>();
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
        .expect("layout")
        .timeline_entries
        .into_iter()
        .find(|e| e.step_id == 1)
        .expect("entry");
    assert_eq!(awaiting.lifecycle, StepLifecycle::AwaitingApproval);
    mode.resolve_pending_approval(true, &ctx);
    let approved = mode
        .task_layout_state()
        .expect("layout")
        .timeline_entries
        .into_iter()
        .find(|e| e.step_id == 1)
        .expect("entry");
    assert_eq!(approved.lifecycle, StepLifecycle::Approved);
    assert_eq!(approved.label, "read_file: approved");
}

#[test]
fn manual_timeline_selection_opens_tool_inspector() {
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
    let state = mode.task_layout_state().expect("layout");
    assert!(state.output_title.starts_with("Inspector"));
    assert!(state.output_title.contains("read_file"));
    assert_eq!(state.output_rows[0].as_display_str(), "Tool: read_file");
}

#[test]
fn follow_mode_auto_advances_and_timeline_down_disables_it() {
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
    let state = mode.task_layout_state().expect("layout");
    assert!(state.follow_mode);
    assert_eq!(state.selected_step, 1);

    mode.timeline_follow_mode = true;
    mode.selected_timeline_index = 1;
    mode.apply_timeline_down(5);
    assert_eq!(mode.selected_timeline_index, 2);
    assert!(!mode.timeline_follow_mode);
    mode.apply_timeline_end(5);
    assert!(mode.timeline_follow_mode);
}
