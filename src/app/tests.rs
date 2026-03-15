use super::*;
use crate::api::{mock_client::MockApiClient, ApiClient};
use crate::ui::editor::{InputAction, InputEditor};
use crossterm::event::KeyEvent;
use futures::FutureExt;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn setup_ctx() -> RuntimeContext {
    let (tx, _rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    RuntimeContext::new(conversation, tx, CancellationToken::new())
}

fn setup_ctx_with_updates() -> (RuntimeContext, mpsc::UnboundedReceiver<UiUpdate>) {
    let (tx, rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    (
        RuntimeContext::new(conversation, tx, CancellationToken::new()),
        rx,
    )
}

fn setup_ctx_with_responses_and_updates(
    responses: Vec<Vec<String>>,
) -> (RuntimeContext, mpsc::UnboundedReceiver<UiUpdate>) {
    let (tx, rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(responses)));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    (
        RuntimeContext::new(conversation, tx, CancellationToken::new()),
        rx,
    )
}

#[test]
fn test_selected_system_prompt_falls_back_to_bundled_prompt() {
    let mut mode = TuiMode::new();
    mode.model_profile.system_prompt = PathBuf::from("src/prompts/missing.txt");

    assert_eq!(mode.selected_system_prompt(), CODER_SYSTEM_PROMPT);
}

fn setup_ctx_with_responses(responses: Vec<Vec<String>>) -> RuntimeContext {
    let (tx, _rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(responses)));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    RuntimeContext::new(conversation, tx, CancellationToken::new())
}

fn config_with_workdir(path: &std::path::Path) -> Config {
    let mut config = Config::default_for_tui();
    config.working_dir = path.to_path_buf();
    config
}

fn write_custom_command(
    dir: &std::path::Path,
    file_name: &str,
    name: &str,
    description: &str,
    template: &str,
) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join(file_name),
        format!("name = {name:?}\ndescription = {description:?}\ntemplate = {template:?}\n"),
    )
    .unwrap();
}

fn successful_run_input() -> String {
    if cfg!(windows) {
        "/run cmd /C exit 0".to_string()
    } else {
        "/run sh -c true".to_string()
    }
}

fn successful_bang_input() -> String {
    "!echo inline-shell".to_string()
}

async fn drain_until_turn_complete(
    mode: &mut TuiMode,
    ctx: &mut RuntimeContext,
    rx: &mut mpsc::UnboundedReceiver<UiUpdate>,
) {
    loop {
        let update = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("timed out waiting for ui update")
            .expect("ui update channel closed");
        let terminal = matches!(update, UiUpdate::TurnComplete | UiUpdate::Error(_));
        mode.on_model_update(update, ctx);
        if terminal && !mode.is_turn_in_progress() {
            break;
        }
    }
}

#[derive(Clone)]
struct RecordingSandbox {
    wrapped: Arc<AtomicBool>,
}

impl SandboxDriver for RecordingSandbox {
    fn wrap(&self, request: CommandRequest) -> Result<CommandRequest> {
        self.wrapped.store(true, Ordering::SeqCst);
        Ok(request)
    }
}

#[tokio::test]
async fn test_ref_03_tui_mode_overlay_blocks_input() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();

    let (response_tx, _rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    mode.on_user_input("blocked".to_string(), &mut ctx);
    assert!(
        !mode.history_state.turn_in_progress,
        "overlay must block input dispatch"
    );

    mode.on_user_input("1".to_string(), &mut ctx);
    assert!(
        !mode.overlay_active(),
        "overlay should clear after decision"
    );

    mode.on_user_input("resume".to_string(), &mut ctx);
    assert!(
        mode.history_state.turn_in_progress,
        "dispatch should resume after overlay clears"
    );
}

#[test]
fn overlay_blocks_submit() {
    let overlay_none = overlay_event_to_user_input(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    assert!(
        overlay_none.is_none(),
        "overlay keymap must not route Enter as normal submit"
    );

    match overlay_event_to_user_input(Event::Key(KeyEvent::new(
        KeyCode::Char('1'),
        KeyModifiers::NONE,
    ))) {
        Some(UserInputEvent::Text(value)) => assert_eq!(value, "1"),
        _ => panic!("overlay key '1' must route to modal action"),
    }

    match overlay_event_to_user_input(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))) {
        Some(UserInputEvent::Text(value)) => assert_eq!(value, "esc"),
        _ => panic!("overlay Esc must route to modal deny action"),
    }
}

#[test]
fn approval_selection_parser_handles_shared_overlay_inputs() {
    assert_eq!(
        parse_approval_selection("1"),
        Some(ApprovalSelection::ApproveOnce)
    );
    assert_eq!(
        parse_approval_selection("yes"),
        Some(ApprovalSelection::ApproveOnce)
    );
    assert_eq!(
        parse_approval_selection("2"),
        Some(ApprovalSelection::ApproveSession)
    );
    assert_eq!(
        parse_approval_selection("always"),
        Some(ApprovalSelection::ApproveSession)
    );
    assert_eq!(parse_approval_selection("3"), Some(ApprovalSelection::Deny));
    assert_eq!(
        parse_approval_selection("esc"),
        Some(ApprovalSelection::Deny)
    );
    assert_eq!(parse_approval_selection("later"), None);
}

#[test]
fn test_ref_08_stream_delta_appends_to_assistant_placeholder_not_user_line() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("hello".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("assistant".to_string()), &mut ctx);

    assert_eq!(mode.history_state.lines[0], "> hello");
    assert_eq!(mode.history_state.lines[1], "assistant");
}

#[test]
fn test_stream_delta_strips_tagged_tool_markup_from_history() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("show diff".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamDelta("I will check.\n<function=git_diff>\n</function>\nDone.".to_string()),
        &mut ctx,
    );

    assert_eq!(mode.history_state.lines[1], "I will check.\n\nDone.");
    assert!(!mode.history_state.lines[1].contains("<function="));
}

#[test]
fn test_stream_delta_hides_incomplete_tool_tag_suffix() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("status".to_string(), &mut ctx);
    mode.on_model_update(
        UiUpdate::StreamDelta("Checking\n<function=git_status".to_string()),
        &mut ctx,
    );

    assert_eq!(mode.history_state.lines[1], "Checking\n");
}

#[test]
fn test_transcript_does_not_exceed_cap_after_n_turns() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var(MAX_HISTORY_LINES_ENV, "10");

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    assert_eq!(mode.history_line_cap, 10);

    for i in 0..20 {
        mode.on_user_input(format!("user-{i}"), &mut ctx);
        assert!(
            mode.history_state.lines.len() <= 10,
            "history must be capped after on_user_input"
        );
        if let Some(idx) = mode.history_state.active_assistant_index {
            assert!(
                idx < mode.history_state.lines.len(),
                "active assistant index must remain valid after cap enforcement"
            );
        }

        mode.on_model_update(UiUpdate::StreamDelta(format!("assistant-{i}")), &mut ctx);
        assert!(
            mode.history_state.lines.len() <= 10,
            "history must be capped after stream update"
        );
        if let Some(idx) = mode.history_state.active_assistant_index {
            assert!(
                idx < mode.history_state.lines.len(),
                "active assistant index must remain valid during streaming"
            );
        }

        mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
        assert!(
            mode.history_state.lines.len() <= 10,
            "history must stay capped after turn completion"
        );
    }

    std::env::remove_var(MAX_HISTORY_LINES_ENV);
}

#[test]
fn test_history_cap_env_invalid_uses_default() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var(MAX_HISTORY_LINES_ENV, "invalid-cap");

    let mode = TuiMode::new();
    assert_eq!(mode.history_line_cap, DEFAULT_MAX_HISTORY_LINES);

    std::env::remove_var(MAX_HISTORY_LINES_ENV);
}

#[test]
fn test_scrollback_retains_position_during_streaming() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.history_state.lines = (0..20).map(|i| format!("line-{i}")).collect();
    mode.history_state.active_assistant_index = Some(10);
    mode.history_state.scroll_offset = 5;
    mode.history_state.auto_follow = false;

    mode.on_model_update(UiUpdate::StreamDelta(" assistant".to_string()), &mut ctx);

    assert_eq!(
        mode.history_state.scroll_offset, 5,
        "scrollback position must not be forced while auto-follow is disabled"
    );
}

#[test]
fn test_scrollback_commands_update_scroll_state() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.history_state.lines = (0..100).map(|i| format!("line-{i}")).collect();
    mode.history_state.scroll_offset = 80;
    mode.history_state.auto_follow = true;

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::History,
            action: ScrollAction::PageUp(10),
        },
        &mut ctx,
    );
    assert_eq!(mode.history_state.scroll_offset, 70);
    assert!(!mode.history_state.auto_follow);

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::History,
            action: ScrollAction::PageDown(200),
        },
        &mut ctx,
    );
    assert_eq!(mode.history_state.scroll_offset, 99);
    assert!(mode.history_state.auto_follow);

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::History,
            action: ScrollAction::Home,
        },
        &mut ctx,
    );
    assert_eq!(mode.history_state.scroll_offset, 0);
    assert!(!mode.history_state.auto_follow);

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::History,
            action: ScrollAction::End,
        },
        &mut ctx,
    );
    assert_eq!(mode.history_state.scroll_offset, 99);
    assert!(mode.history_state.auto_follow);
    assert!(
        !mode.history_state.turn_in_progress,
        "scroll commands must not dispatch new turns"
    );
}

#[test]
fn test_history_status_and_scroll_use_visual_rows() {
    let mode = TuiMode {
        history_state: HistoryState {
            lines: vec!["a\nb\nc".to_string()],
            ..HistoryState::default()
        },
        ..TuiMode::new()
    };

    assert_eq!(mode.max_scroll_offset(), 2);
    assert!(mode.status_line().contains("history:3"));
}

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
        mode.current_turn_changed_files.is_empty(),
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
    assert!(mode.current_turn_changed_files.contains("src/main.rs"));
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
        mode.current_turn_changed_files.is_empty(),
        "failed tool calls must not be exported as changed files"
    );
}

#[test]
fn test_idle_interrupt_shows_feedback() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    assert!(!mode.history_state.turn_in_progress);
    assert!(!mode.pending_quit);
    assert!(!mode.quit_requested);

    mode.on_interrupt(&mut ctx);
    assert!(mode.pending_quit, "first idle interrupt must arm quit");
    assert!(!mode.quit_requested, "first idle interrupt must not quit");
    assert!(
        mode.history_state
            .lines
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

    mode.history_state.turn_in_progress = true;
    mode.on_user_input("hello".to_string(), &mut ctx);

    assert!(
        mode.history_state.turn_in_progress,
        "busy input must not start a new turn"
    );
    assert!(
        mode.history_state
            .lines
            .iter()
            .any(|line| line.starts_with("[busy")),
        "busy input must produce visible rejection feedback"
    );
    assert!(
        !mode
            .history_state
            .lines
            .iter()
            .any(|line| line == "> hello"),
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
    assert!(mode.history_state.turn_in_progress);
}

#[test]
fn overlay_renders_after_base_panes() {
    let mode = TuiMode::new();
    assert_eq!(
        render_pass_order(&mode),
        vec![RenderPass::Header, RenderPass::History, RenderPass::Input]
    );

    let mut overlay_mode = TuiMode::new();
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
    overlay_mode.overlay_state.pending_approval = Some(PendingApproval {
        tool_name: "read_file".to_string(),
        input_preview: "{\"path\":\"Cargo.toml\"}".to_string(),
        action: PendingApprovalAction::Tool(response_tx),
    });
    assert_eq!(
        render_pass_order(&overlay_mode),
        vec![
            RenderPass::Header,
            RenderPass::History,
            RenderPass::Input,
            RenderPass::Overlay,
        ],
        "overlay must always render last"
    );
}

#[test]
fn test_render_not_called_when_state_unchanged() {
    let start = Instant::now();
    let mut guard = RenderGuard::with_intervals(
        Duration::from_millis(500),
        Duration::from_millis(120),
        start,
    );

    assert!(
        guard.should_draw(start, 11),
        "first render should draw because the guard starts dirty"
    );
    assert!(
        !guard.should_draw(start + Duration::from_millis(20), 11),
        "unchanged state before tick interval must not draw"
    );
    assert!(
        !guard.should_draw(start + Duration::from_millis(100), 11),
        "unchanged state still below tick interval must not draw"
    );
    assert!(
        guard.should_draw(start + Duration::from_millis(121), 11),
        "unchanged state should draw when tick interval elapses"
    );
    assert!(
        guard.should_draw(start + Duration::from_millis(122), 12),
        "changed state should mark dirty and draw immediately"
    );
}

#[test]
fn test_render_guard_poll_timeout_uses_min_tick_interval() {
    let guard = RenderGuard::with_intervals(
        Duration::from_millis(500),
        Duration::from_millis(120),
        Instant::now(),
    );
    assert_eq!(guard.poll_timeout(), Duration::from_millis(120));
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
    assert_eq!(
        render_pass_order(&mode).first(),
        Some(&RenderPass::Header),
        "header row must remain first in render order"
    );

    mode.on_user_input("hello".to_string(), &mut ctx);
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
    assert_eq!(
        render_pass_order(&mode).first(),
        Some(&RenderPass::Header),
        "header row must remain first while streaming"
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
    assert_eq!(
        render_pass_order(&mode).first(),
        Some(&RenderPass::Header),
        "header row must remain first under overlay"
    );
}

#[test]
fn multiline_submit_outside_overlay_only() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    let mut editor = InputEditor::new();

    editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
    editor.apply_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

    let submitted = match editor.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
        InputAction::Submit(value) => value,
        _ => panic!("enter outside overlay must submit multiline buffer"),
    };
    assert_eq!(submitted, "a\nb\nc");

    mode.on_user_input(submitted.clone(), &mut ctx);
    assert!(
        mode.history_state.turn_in_progress,
        "outside overlay, enter must submit and start a turn"
    );
    assert!(
        mode.history_state
            .lines
            .iter()
            .any(|line| line == "> a\nb\nc"),
        "submitted multiline prompt should be recorded in history"
    );

    mode.history_state.turn_in_progress = false;
    mode.history_state.active_assistant_index = None;
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.overlay_state.pending_approval = Some(PendingApproval {
        tool_name: "read_file".to_string(),
        input_preview: "{}".to_string(),
        action: PendingApprovalAction::Tool(response_tx),
    });

    let overlay_enter = overlay_event_to_user_input(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    assert!(
        overlay_enter.is_none(),
        "enter in overlay keymap must not route to submit"
    );

    mode.on_user_input("overlay\nattempt".to_string(), &mut ctx);
    assert!(
        mode.overlay_active(),
        "overlay should remain active after non-decision input"
    );
    assert!(
        !mode
            .history_state
            .lines
            .iter()
            .any(|line| line == "> overlay\nattempt"),
        "overlay-focused input must not submit as a user prompt"
    );
}

#[test]
fn history_stable_during_overlay() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    let mut editor = InputEditor::new();

    editor.input_state.buffer = "first".to_string();
    let _ = editor.submit();
    editor.input_state.buffer = "second".to_string();
    let _ = editor.submit();
    editor.input_state.buffer = "draft".to_string();
    editor.input_state.cursor = editor.input_state.buffer.len();

    editor.history_up();
    let before_overlay_buffer = editor.input_state.buffer.clone();
    let before_overlay_index = editor.input_state.history_index;
    let before_overlay_history_len = editor.input_state.history.len();

    let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.overlay_state.pending_approval = Some(PendingApproval {
        tool_name: "read_file".to_string(),
        input_preview: "{}".to_string(),
        action: PendingApprovalAction::Tool(response_tx),
    });
    assert!(mode.overlay_active());

    let up =
        overlay_event_to_user_input(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
    let down =
        overlay_event_to_user_input(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
    assert!(
        up.is_none(),
        "overlay keymap must consume history navigation"
    );
    assert!(
        down.is_none(),
        "overlay keymap must consume history navigation"
    );

    assert_eq!(editor.input_state.buffer, before_overlay_buffer);
    assert_eq!(editor.input_state.history_index, before_overlay_index);
    assert_eq!(editor.input_state.history.len(), before_overlay_history_len);

    mode.on_user_input("1".to_string(), &mut ctx);
    assert!(!mode.overlay_active(), "overlay should clear on decision");

    editor.history_down();
    assert_eq!(editor.input_state.history_index, None);
    assert_eq!(
        editor.input_state.buffer, "draft",
        "prompt draft must restore after overlay transition"
    );
}

#[tokio::test]
async fn diff_overlay_scrolls() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    let patch_preview = [
        "@@ -1,3 +1,4".to_string(),
        " context line".to_string(),
        "-old value".to_string(),
        "+new value".to_string(),
        " context tail".to_string(),
        "-removed again".to_string(),
        "+added again".to_string(),
    ]
    .join("\n");

    let (approve_tx, approve_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.overlay_state.pending_patch_approval = Some(PendingPatchApproval {
        patch_preview: patch_preview.clone(),
        scroll_offset: 0,
        response_tx: Some(approve_tx),
    });

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::Overlay,
            action: ScrollAction::LineDown,
        },
        &mut ctx,
    );
    assert_eq!(
        mode.overlay_state
            .pending_patch_approval
            .as_ref()
            .map(|p| p.scroll_offset),
        Some(1),
        "down must advance diff overlay scroll"
    );

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::Overlay,
            action: ScrollAction::PageDown(3),
        },
        &mut ctx,
    );
    assert_eq!(
        mode.overlay_state
            .pending_patch_approval
            .as_ref()
            .map(|p| p.scroll_offset),
        Some(4),
        "page down must advance by requested step"
    );

    mode.on_frontend_event(
        UserInputEvent::Scroll {
            target: ScrollTarget::Overlay,
            action: ScrollAction::End,
        },
        &mut ctx,
    );
    assert_eq!(
        mode.overlay_state
            .pending_patch_approval
            .as_ref()
            .map(|p| p.scroll_offset),
        Some(patch_preview.lines().count().saturating_sub(1)),
        "end must jump to last diff line"
    );

    mode.on_user_input("1".to_string(), &mut ctx);
    assert!(
        approve_rx.await.expect("patch approval should resolve"),
        "approve binding must resolve true"
    );
    assert!(
        !mode.patch_overlay_active(),
        "overlay must clear after approve decision"
    );

    let (deny_tx, deny_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.overlay_state.pending_patch_approval = Some(PendingPatchApproval {
        patch_preview,
        scroll_offset: 2,
        response_tx: Some(deny_tx),
    });
    mode.on_user_input("n".to_string(), &mut ctx);
    assert!(
        !deny_rx.await.expect("patch denial should resolve"),
        "deny binding must resolve false"
    );
    assert!(
        !mode.patch_overlay_active(),
        "overlay must clear after deny decision"
    );
}

#[test]
fn input_pane_expands_then_clamps_to_max_rows() {
    assert_eq!(input_rows_for_buffer("", 80), 1);

    let multiline = (0..12)
        .map(|idx| format!("line-{idx}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        input_rows_for_buffer(&multiline, 80),
        MAX_INPUT_PANE_ROWS as u16
    );
}

#[test]
fn test_editor_cursor_navigation() {
    let mut editor = InputEditor::new();
    editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
    assert_eq!(editor.input_state.buffer, "aXbc");
}

#[test]
fn test_editor_history_up_down() {
    let mut editor = InputEditor::new();
    editor.input_state.buffer = "first".to_string();
    let _ = editor.submit();
    editor.input_state.buffer = "second".to_string();
    let _ = editor.submit();

    editor.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(editor.input_state.buffer, "second");
    editor.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(editor.input_state.buffer, "first");
    editor.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(editor.input_state.buffer, "second");
}

#[test]
fn test_editor_history_stash_restore() {
    let mut editor = InputEditor::new();

    editor.input_state.buffer = "first".to_string();
    let _ = editor.submit();
    editor.input_state.buffer = "second".to_string();
    let _ = editor.submit();

    editor.input_state.buffer = "draft".to_string();
    editor.input_state.cursor = editor.input_state.buffer.len();

    editor.history_up();
    assert_eq!(editor.input_state.buffer, "second");
    assert_eq!(editor.input_state.history_index, Some(1));

    editor.history_down();
    assert_eq!(editor.input_state.history_index, None);
    assert_eq!(editor.input_state.buffer, "draft");
    assert_eq!(editor.input_state.cursor, "draft".len());
}

#[test]
fn test_editor_multiline_shortcuts() {
    let mut editor = InputEditor::new();
    editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
    editor.apply_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
    assert_eq!(editor.input_state.buffer, "a\nb\nc");
}

#[test]
fn test_editor_undo_redo() {
    let mut editor = InputEditor::new();
    editor.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    editor.apply_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(editor.input_state.buffer, "a");
    editor.apply_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
    assert_eq!(editor.input_state.buffer, "ab");
}

#[test]
fn test_editor_paste_handling() {
    let mut editor = InputEditor::new();
    let _ = editor.apply_event(Event::Paste("hello".to_string()));
    assert_eq!(editor.input_state.buffer, "hello");
}

#[test]
fn test_input_editor_unicode_cursor_backspace_delete_safe() {
    let mut editor = InputEditor::new();
    editor.insert_str("a\u{1F600}b");
    editor.input_state.cursor = editor.input_state.buffer.len();
    editor.backspace();
    assert_eq!(editor.input_state.buffer, "a\u{1F600}");
    editor.backspace();
    assert_eq!(editor.input_state.buffer, "a");

    editor.insert_str("\u{1F600}b");
    editor.input_state.cursor = 2; // intentionally non-boundary (inside emoji codepoint)
    editor.delete();
    assert_eq!(editor.input_state.buffer, "ab");
}

#[tokio::test]
async fn test_invalid_approval_input_keeps_overlay_active_with_feedback() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel::<bool>();

    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    mode.on_user_input("x".to_string(), &mut ctx);
    assert!(
        mode.overlay_active(),
        "overlay should stay active on invalid input"
    );
    assert!(
        mode.history_state
            .lines
            .iter()
            .any(|line| line.contains("[invalid selection, expected 1/2/3]")),
        "expected invalid selection feedback line"
    );
}

#[tokio::test]
async fn test_interrupt_is_typed_event_not_magic_string_collision() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();

    mode.on_user_input("__VEX_INTERRUPT__".to_string(), &mut ctx);
    assert!(
        mode.history_state.turn_in_progress,
        "plain text matching old sentinel must be treated as normal user input"
    );

    mode.on_interrupt(&mut ctx);
    assert!(
        mode.history_state.turn_in_progress,
        "typed interrupt should keep turn active until TurnComplete drains"
    );
    assert!(
        mode.history_state.cancel_pending,
        "typed interrupt should arm cancel-pending state"
    );
    assert!(
        mode.history_state
            .lines
            .iter()
            .any(|line| line.contains("[turn cancellation requested]")),
        "cancel path should provide visible feedback"
    );

    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
    assert!(!mode.history_state.turn_in_progress);
    assert!(!mode.history_state.cancel_pending);
}

#[test]
fn test_stream_delta_ignored_without_active_turn_slot() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_model_update(UiUpdate::StreamDelta("ghost delta".to_string()), &mut ctx);
    assert!(
        mode.history_state.lines.is_empty(),
        "stale stream deltas must be ignored after turn completion/cancel"
    );
}

#[test]
fn test_cancel_pending_blocks_stream_delta_appends() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("hello".to_string(), &mut ctx);
    mode.on_interrupt(&mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("stale".to_string()), &mut ctx);
    assert_eq!(mode.history_state.lines[0], "> hello");
    assert_eq!(mode.history_state.lines[1], "");
}

#[tokio::test]
async fn test_tool_approval_accept_once() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );
    mode.on_user_input("1".to_string(), &mut ctx);

    assert!(response_rx.await.expect("response should resolve"));
}

#[tokio::test]
async fn test_tool_approval_deny() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "{}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );
    mode.on_user_input("n".to_string(), &mut ctx);

    assert!(!response_rx.await.expect("response should resolve"));
}

#[tokio::test]
async fn approval_sender_resolved_exactly_once() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();

    let (first_tx, first_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "read_file".to_string(),
            input_preview: "first".to_string(),
            response_tx: first_tx,
        }),
        &mut ctx,
    );

    let mut first_rx = Box::pin(first_rx);
    assert!(
        first_rx.as_mut().now_or_never().is_none(),
        "first approval sender must remain unresolved while overlay is active"
    );

    let (second_tx, second_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "write_file".to_string(),
            input_preview: "second".to_string(),
            response_tx: second_tx,
        }),
        &mut ctx,
    );

    assert!(
        !first_rx
            .await
            .expect("first sender should resolve when replaced"),
        "replaced approval sender must resolve false exactly once"
    );

    let mut second_rx = Box::pin(second_rx);
    assert!(
        second_rx.as_mut().now_or_never().is_none(),
        "second approval sender must remain unresolved before decision"
    );

    mode.on_user_input("1".to_string(), &mut ctx);
    assert!(
        second_rx
            .await
            .expect("second sender should resolve on accept"),
        "approved overlay should resolve true exactly once"
    );

    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
    mode.on_model_update(UiUpdate::Error("post-resolution".to_string()), &mut ctx);
    assert!(
        !mode.overlay_active(),
        "overlay lifecycle should clear cleanly after sender resolution"
    );
}

#[test]
fn test_tui_memory_renders_empty_notes() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(Some(notes_path));
    mode.on_user_input("/memory".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[memory] no notes")),
        "expected '[memory] no notes' in history"
    );
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_tui_memory_add_appends_to_file() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));
    mode.on_user_input("/memory add hello world".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[memory: note added]")),
        "expected '[memory: note added]' in history"
    );
    let content = std::fs::read_to_string(&notes_path).unwrap();
    assert!(content.contains("hello world"));
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_tui_memory_clear_requires_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    std::fs::write(&notes_path, "existing note\n").unwrap();
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));
    mode.on_user_input("/memory clear".to_string(), &mut ctx);
    assert!(
        mode.pending_memory_clear_overlay(),
        "memory clear must enter overlay state"
    );
    assert!(
        mode.overlay_active(),
        "overlay must be active during memory clear"
    );
    // File must not be cleared until confirmed
    let content = std::fs::read_to_string(&notes_path).unwrap();
    assert!(content.contains("existing note"));
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_tui_memory_clear_cancellable() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    std::fs::write(&notes_path, "keep this note\n").unwrap();
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));
    mode.on_user_input("/memory clear".to_string(), &mut ctx);
    mode.on_user_input("n".to_string(), &mut ctx);
    assert!(
        !mode.pending_memory_clear_overlay(),
        "overlay must clear after cancel"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[memory: cancelled]")),
        "expected '[memory: cancelled]' in history"
    );
    let content = std::fs::read_to_string(&notes_path).unwrap();
    assert!(
        content.contains("keep this note"),
        "file must not be cleared on cancel"
    );
}

#[test]
fn test_tui_memory_does_not_call_start_turn() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    std::fs::write(&notes_path, "a note\n").unwrap();
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(Some(notes_path.clone()));

    // /memory
    mode.on_user_input("/memory".to_string(), &mut ctx);
    assert!(!mode.is_turn_in_progress(), "/memory must not start a turn");

    // /memory add
    mode.on_user_input("/memory add another".to_string(), &mut ctx);
    assert!(
        !mode.is_turn_in_progress(),
        "/memory add must not start a turn"
    );

    // /memory clear + cancel
    mode.on_user_input("/memory clear".to_string(), &mut ctx);
    assert!(
        !mode.is_turn_in_progress(),
        "/memory clear must not start a turn"
    );
    mode.on_user_input("n".to_string(), &mut ctx);
    assert!(!mode.is_turn_in_progress(), "cancel must not start a turn");
}

#[test]
fn test_tui_memory_reads_legacy_fallback_notes() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir_all(home.join(".vex")).unwrap();
    std::fs::write(home.join(".vex/memory.md"), "legacy note\n").unwrap();

    let old_home = std::env::var("HOME").ok();
    let old_xdg = std::env::var("XDG_CONFIG_HOME").ok();
    std::env::set_var("HOME", home.as_os_str());
    std::env::remove_var("XDG_CONFIG_HOME");

    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new_with_notes(None);
    mode.on_user_input("/memory".to_string(), &mut ctx);

    match old_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    match old_xdg {
        Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
        None => std::env::remove_var("XDG_CONFIG_HOME"),
    }

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("legacy note")),
        "expected legacy fallback notes to render"
    );
}

#[test]
fn test_memory_injection_within_budget_returns_content() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    std::fs::write(&notes_path, "my project note\n").unwrap();
    let (content, warning) = resolve_notes_for_injection(Some(notes_path.as_path()), 2048);
    assert!(warning.is_none(), "notes within budget must not warn");
    let content = content.as_deref().unwrap_or("");
    assert!(
        content.contains("my project note"),
        "notes content must be returned for system prompt injection"
    );
}

#[test]
fn test_memory_injection_over_budget_emits_startup_warning() {
    let temp = tempfile::tempdir().unwrap();
    let notes_path = temp.path().join("memory.md");
    let big_content = "x".repeat((2048 * 4) + 1);
    std::fs::write(&notes_path, &big_content).unwrap();

    let config = Config {
        model_token: None,
        model_name: "mock-model".to_string(),
        model_url: "http://localhost:8000/v1/messages".to_string(),
        working_dir: temp.path().to_path_buf(),
        model_backend: crate::runtime::ModelBackendKind::LocalRuntime,
        model_protocol: crate::runtime::ModelProtocol::MessagesV1,
        tool_call_mode: crate::runtime::ToolCallMode::TaggedFallback,
        model_profile: ModelProfile::default_for_backend(
            crate::runtime::ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        model_headers: reqwest::header::HeaderMap::new(),
        notes_path: Some(notes_path),
        hooks: Vec::new(),
    };

    let (runtime, _ctx) = build_runtime(config).expect("runtime should build");
    let has_warning = runtime
        .mode
        .history_lines()
        .iter()
        .any(|l| l.contains("notes exceed token budget"));
    assert!(has_warning, "expected startup budget warning in history");
}

// -- PI-04 / PI-05 / PJ-01 / PJ-02 ---------------------------------------

#[test]
fn test_tui_new_saves_current_state_before_reset() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let original_id = mode.current_task_id();
    let mut ctx = setup_ctx();

    mode.on_user_input("/new".to_string(), &mut ctx);

    let state_file = temp.path().join(format!("{original_id}.json"));
    assert!(state_file.exists(), "/new must save the prior task state");
    assert_eq!(
        mode.history_lines().len(),
        1,
        "/new must reset the transcript"
    );
    assert!(
        mode.history_lines()[0].starts_with("[new session: task-"),
        "expected new-session marker, got: {:?}",
        mode.history_lines()
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_new_creates_fresh_task_id() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let original_id = mode.current_task_id();
    let mut ctx = setup_ctx();
    mode.on_user_input("/new".to_string(), &mut ctx);

    assert_ne!(
        mode.current_task_id(),
        original_id,
        "/new must assign a new task-id"
    );
    assert!(
        !mode.is_turn_in_progress(),
        "/new must not leave a stale turn active"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_new_clears_active_edit_loop() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/new".to_string(), &mut ctx);

    assert!(
        !mode.is_turn_in_progress(),
        "/new must clear active edit-loop state"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_resume_restores_active_grants() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut saved = TaskState::new("task-resume-001".to_string());
    saved.active_grants.insert(
        crate::runtime::Capability::ApplyPatch,
        crate::runtime::ApprovalScope::Session,
    );
    saved.changed_files.push(PathBuf::from("src/app.rs"));
    saved.status = crate::runtime::TaskStatus::Completed;
    saved.save(temp.path()).unwrap();

    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();
    mode.on_user_input("/resume task-resume-001".to_string(), &mut ctx);

    assert_eq!(mode.current_task_id(), "task-resume-001");
    assert!(mode
        .current_task
        .active_grants
        .contains_key(&crate::runtime::Capability::ApplyPatch));
    assert_eq!(
        mode.current_task.changed_files,
        vec![PathBuf::from("src/app.rs")]
    );
    assert_eq!(
        mode.current_task.status,
        crate::runtime::TaskStatus::Completed
    );
    assert_eq!(
        mode.history_lines().len(),
        1,
        "/resume must reset the transcript"
    );
    assert!(
        mode.history_lines()[0].contains("[resumed: task-resume-001 status=Completed]"),
        "expected resume confirmation in history"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_resume_without_id_offers_recent_task_selection() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let older = TaskState::new("task-resume-older".to_string());
    older.save(temp.path()).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    let mut newer = TaskState::new("task-resume-newer".to_string());
    newer.status = crate::runtime::TaskStatus::Running;
    newer.save(temp.path()).unwrap();

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/resume".to_string(), &mut ctx);

    assert!(
        mode.overlay_active(),
        "/resume without id must open a selection overlay"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("task-resume-newer (Running)")),
        "expected recent-task list in history"
    );

    mode.on_user_input("1".to_string(), &mut ctx);

    assert_eq!(mode.current_task_id(), "task-resume-newer");
    assert_eq!(
        mode.history_lines().len(),
        1,
        "resume selection must reset transcript"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_resume_does_not_restore_conversation() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let saved = TaskState::new("task-resume-002".to_string());
    saved.save(temp.path()).unwrap();

    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();
    mode.on_user_input("/resume task-resume-002".to_string(), &mut ctx);

    assert_eq!(
        mode.history_lines().len(),
        1,
        "/resume must clear prior transcript state"
    );
    assert!(
        !mode.is_turn_in_progress(),
        "/resume must not start a model turn"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_resume_unknown_id_emits_error() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/resume task-does-not-exist".to_string(), &mut ctx);

    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[resume: task 'task-does-not-exist' not found]")),
        "expected not-found message in history"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_resume_restores_legacy_subdir_state() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let old_cwd = std::env::current_dir().unwrap();
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".git")).unwrap();
    let nested = temp.path().join("src/nested");
    let legacy_state_dir = nested.join(".vex/state");
    std::fs::create_dir_all(&legacy_state_dir).unwrap();

    let mut saved = TaskState::new("task-legacy-ui".to_string());
    saved.status = crate::runtime::TaskStatus::Completed;
    saved.save(&legacy_state_dir).unwrap();

    std::env::remove_var("VEX_STATE_DIR");
    std::env::set_current_dir(&nested).unwrap();

    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/resume task-legacy-ui".to_string(), &mut ctx);

    std::env::set_current_dir(old_cwd).unwrap();

    assert_eq!(mode.current_task_id(), "task-legacy-ui");
    assert!(
        mode.history_lines()[0].contains("[resumed: task-legacy-ui status=Completed]"),
        "expected resume confirmation in history"
    );
}

#[test]
fn test_tui_clear_resets_conversation_history() {
    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();

    mode.on_user_input("/clear".to_string(), &mut ctx);

    assert_eq!(
        mode.history_lines().len(),
        1,
        "/clear must reset the transcript"
    );
    assert!(
        mode.history_lines()[0].starts_with("[cleared: conversation history reset; task "),
        "expected cleared confirmation"
    );
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_tui_clear_preserves_task_id_and_grants() {
    let mut mode = TuiMode::new();
    let original_id = mode.current_task_id();
    mode.current_task.active_grants.insert(
        crate::runtime::Capability::RunCommand,
        crate::runtime::ApprovalScope::Session,
    );
    let mut ctx = setup_ctx();

    mode.on_user_input("/clear".to_string(), &mut ctx);

    assert_eq!(
        mode.current_task_id(),
        original_id,
        "/clear must not change task-id"
    );
    assert!(
        mode.current_task
            .active_grants
            .contains_key(&crate::runtime::Capability::RunCommand),
        "/clear must preserve active grants"
    );
    assert!(
        !mode.is_turn_in_progress(),
        "/clear must clear active edit-loop state"
    );
}

#[test]
fn test_tui_clear_clears_active_edit_loop() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/clear".to_string(), &mut ctx);

    assert!(
        !mode.is_turn_in_progress(),
        "/clear must clear active edit-loop state"
    );
}

#[test]
fn test_tui_fork_saves_parent_before_branching() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let parent_id = mode.current_task_id();
    let mut ctx = setup_ctx();
    mode.on_user_input("/fork".to_string(), &mut ctx);

    let parent_file = temp.path().join(format!("{parent_id}.json"));
    assert!(parent_file.exists(), "/fork must save parent state file");
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_fork_creates_new_task_id() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    let parent_id = mode.current_task_id();
    mode.current_task.active_grants.insert(
        crate::runtime::Capability::RunCommand,
        crate::runtime::ApprovalScope::Session,
    );
    mode.current_task
        .changed_files
        .push(PathBuf::from("src/app.rs"));
    mode.current_task.status = crate::runtime::TaskStatus::Running;
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();

    mode.on_user_input("/fork feature work".to_string(), &mut ctx);

    assert_ne!(
        mode.current_task_id(),
        parent_id,
        "/fork must assign a new task-id"
    );
    assert!(mode.current_task_id().ends_with("-feature-work"));
    assert!(mode
        .current_task
        .active_grants
        .contains_key(&crate::runtime::Capability::RunCommand));
    assert_eq!(
        mode.current_task.changed_files,
        vec![PathBuf::from("src/app.rs")]
    );
    assert_eq!(
        mode.current_task.status,
        crate::runtime::TaskStatus::Running
    );
    assert_eq!(mode.history_lines().len(), 1, "/fork must reset transcript");
    assert!(
        mode.history_lines()[0].contains(&format!("branched from {parent_id}")),
        "expected fork confirmation in history"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_fork_does_not_copy_conversation() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    mode.push_history_line("stale transcript".to_string());
    let mut ctx = setup_ctx();
    mode.on_user_input("/fork".to_string(), &mut ctx);

    assert_eq!(
        mode.history_lines().len(),
        1,
        "/fork must clear prior transcript state"
    );
    assert!(
        !mode.is_turn_in_progress(),
        "/fork must not start a model turn"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_fork_aborts_on_save_failure() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let blocking_path = temp.path().join("state-file");
    std::fs::write(&blocking_path, "occupied").unwrap();
    std::env::set_var("VEX_STATE_DIR", blocking_path.as_os_str());

    let mut mode = TuiMode::new();
    let original_id = mode.current_task_id();
    let mut ctx = setup_ctx();
    mode.on_user_input("/fork".to_string(), &mut ctx);

    assert_eq!(
        mode.current_task_id(),
        original_id,
        "/fork must not change task-id when parent save fails"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[fork] save failed")),
        "expected save failure message"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

// -- PK-01: /quit and /exit ------------------------------------------------

#[test]
fn test_tui_quit_command_requests_quit() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/quit".to_string(), &mut ctx);
    assert!(
        mode.quit_requested(),
        "/quit must set quit_requested immediately"
    );
    assert!(
        !mode.history_state.turn_in_progress,
        "/quit must not start a model turn"
    );
}

#[test]
fn test_tui_exit_is_alias_for_quit() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/exit".to_string(), &mut ctx);
    assert!(
        mode.quit_requested(),
        "/exit must behave identically to /quit"
    );
    assert!(
        !mode.history_state.turn_in_progress,
        "/exit must not start a model turn"
    );
}

// -- PK-02: /about ---------------------------------------------------------

#[test]
fn test_tui_about_renders_without_model_turn() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/about".to_string(), &mut ctx);
    assert!(
        !mode.history_state.turn_in_progress,
        "/about must not start a model turn"
    );
    let has_version = mode
        .history_state
        .lines
        .iter()
        .any(|l| l.starts_with("vex "));
    assert!(has_version, "/about must render version line");
    let has_build = mode.history_state.lines.iter().any(|l| l.contains("build"));
    assert!(has_build, "/about must render build metadata");
    let has_commit = mode
        .history_state
        .lines
        .iter()
        .any(|l| l.contains("commit"));
    assert!(has_commit, "/about must render commit metadata");
}

// -- PI-01 / PI-02 / PI-03 -------------------------------------------------

#[test]
fn test_permissions_empty_grants() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/permissions".to_string(), &mut ctx);
    assert!(
        mode.history_lines().iter().any(|l| l == "[permissions]"),
        "expected permissions header"
    );
    for &cap in ALL_CAPABILITIES {
        let cap_name = capability_to_kebab(cap);
        assert!(
            mode.history_lines()
                .iter()
                .any(|l| l.contains(cap_name) && l.contains("(none)")),
            "expected {cap_name} with (none) in empty-grants permissions output"
        );
    }
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_permissions_lists_active_grants() {
    let mut mode = TuiMode::new();
    mode.current_task
        .active_grants
        .insert(Capability::RunCommand, ApprovalScope::Session);
    mode.current_task
        .active_grants
        .insert(Capability::Network, ApprovalScope::Once);
    let mut ctx = setup_ctx();
    mode.on_user_input("/permissions".to_string(), &mut ctx);
    let lines = mode.history_lines().to_vec();
    let has_header = lines.iter().any(|l| l == "[permissions]");
    let has_run_command = lines
        .iter()
        .any(|l| l.contains("run-command") && l.contains("session"));
    let has_network = lines
        .iter()
        .any(|l| l.contains("network") && l.contains("once"));
    let has_apply_patch_none = lines
        .iter()
        .any(|l| l.contains("apply-patch") && l.contains("(none)"));
    assert!(has_header, "expected active grants header");
    assert!(has_run_command, "expected run-command session entry");
    assert!(has_network, "expected network once entry");
    assert!(
        has_apply_patch_none,
        "expected apply-patch (none) for absent grant"
    );
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_allow_inserts_grant() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/allow run-command session".to_string(), &mut ctx);
    assert_eq!(
        mode.current_task.active_grants.get(&Capability::RunCommand),
        Some(&ApprovalScope::Session),
        "allow must insert the grant with session scope"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[allow: run-command granted for session]")),
        "expected grant confirmation"
    );
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_allow_defaults_to_once_scope() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/allow write-file".to_string(), &mut ctx);
    assert_eq!(
        mode.current_task.active_grants.get(&Capability::WriteFile),
        Some(&ApprovalScope::Once),
        "allow without scope must default to once"
    );
}

#[test]
fn test_allow_unknown_capability_emits_error() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/allow bogus-cap".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[allow: unknown capability 'bogus-cap']")),
        "expected unknown-capability error"
    );
    assert!(mode.current_task.active_grants.is_empty());
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_allow_task_scope_emits_error() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/allow network task".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[allow: unknown scope 'task'; valid: once | session]")),
        "expected task scope rejection"
    );
    assert!(mode.current_task.active_grants.is_empty());
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_allow_unknown_scope_emits_error() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/allow network forever".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[allow: unknown scope 'forever'; valid: once | session]")),
        "expected unknown-scope error"
    );
    assert!(mode.current_task.active_grants.is_empty());
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_deny_removes_grant() {
    let mut mode = TuiMode::new();
    mode.current_task
        .active_grants
        .insert(Capability::ApplyPatch, ApprovalScope::Task);
    let mut ctx = setup_ctx();
    mode.on_user_input("/deny apply-patch".to_string(), &mut ctx);
    assert!(
        !mode
            .current_task
            .active_grants
            .contains_key(&Capability::ApplyPatch),
        "deny must remove the grant"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[deny: apply-patch removed]")),
        "expected revoke confirmation"
    );
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_deny_no_grant_emits_info() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/deny browser".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[deny: browser not in active grants]")),
        "expected no-active-grant info message"
    );
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_deny_unknown_capability_emits_error() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/deny not-a-thing".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[deny: unknown capability 'not-a-thing']")),
        "expected unknown-capability error"
    );
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_capability_kebab_round_trip() {
    for &cap in ALL_CAPABILITIES {
        let kebab = capability_to_kebab(cap);
        let round_tripped = kebab_to_capability(kebab);
        assert_eq!(
            round_tripped,
            Some(cap),
            "capability {kebab} failed round-trip through kebab_to_capability"
        );
    }
}

#[test]
fn test_capability_for_tool_name_maps_builtin_tools() {
    assert_eq!(
        capability_for_tool_name("read_file"),
        Some(Capability::ReadFile)
    );
    assert_eq!(
        capability_for_tool_name("write_file"),
        Some(Capability::WriteFile)
    );
    assert_eq!(
        capability_for_tool_name("apply_patch"),
        Some(Capability::ApplyPatch)
    );
    assert_eq!(
        capability_for_tool_name("run_command"),
        Some(Capability::RunCommand)
    );
    assert_eq!(
        capability_for_tool_name("git_commit"),
        Some(Capability::ApplyPatch)
    );
    assert_eq!(capability_for_tool_name("unknown_tool"), None);
}

#[tokio::test]
async fn test_tool_approval_auto_approves_matching_session_grant() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    mode.current_task
        .active_grants
        .insert(Capability::RunCommand, ApprovalScope::Session);
    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "run_command".to_string(),
            input_preview: "{\"tool\":\"write_file\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    assert!(response_rx.await.expect("response should resolve"));
    assert_eq!(
        mode.current_task.active_grants.get(&Capability::RunCommand),
        Some(&ApprovalScope::Session),
        "session grant must remain after auto-approval"
    );
    assert!(
        mode.overlay_state.pending_approval.is_none(),
        "matching grant must not open the approval overlay"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("[auto-approved tool: run_command session grant]")),
        "expected auto-approval transcript entry"
    );
}

#[tokio::test]
async fn test_tool_approval_consumes_matching_once_grant() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    mode.current_task
        .active_grants
        .insert(Capability::ApplyPatch, ApprovalScope::Once);
    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "apply_patch".to_string(),
            input_preview: "{\"path\":\"src/app.rs\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    assert!(response_rx.await.expect("response should resolve"));
    assert!(
        !mode
            .current_task
            .active_grants
            .contains_key(&Capability::ApplyPatch),
        "once grant must be consumed after auto-approval"
    );
    assert!(
        mode.overlay_state.pending_approval.is_none(),
        "matching once grant must not open the approval overlay"
    );
}

#[tokio::test]
async fn test_tool_approval_prompts_when_grant_does_not_match_tool() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    mode.current_task
        .active_grants
        .insert(Capability::ApplyPatch, ApprovalScope::Session);
    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();

    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "run_command".to_string(),
            input_preview: "{\"tool\":\"write_file\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    let mut response_rx = Box::pin(response_rx);
    assert!(
        response_rx.as_mut().now_or_never().is_none(),
        "non-matching grant must leave approval unresolved"
    );
    assert!(
        mode.overlay_state.pending_approval.is_some(),
        "non-matching grant must still open the approval overlay"
    );
    assert_eq!(
        mode.current_task.active_grants.get(&Capability::ApplyPatch),
        Some(&ApprovalScope::Session),
        "non-matching grant must remain intact"
    );
}

// -- PM-01 (app side): build_runtime_with_resume ---------------------------

#[test]
fn test_build_runtime_with_resume_restores_task() {
    let temp = tempfile::tempdir().unwrap();
    let mut state = TaskState::new("task-startup-resume".to_string());
    state
        .active_grants
        .insert(Capability::Network, ApprovalScope::Session);
    state.status = crate::runtime::TaskStatus::Running;

    let config = Config {
        model_token: None,
        model_name: "mock-model".to_string(),
        model_url: "http://localhost:8000/v1/messages".to_string(),
        working_dir: temp.path().to_path_buf(),
        model_backend: crate::runtime::ModelBackendKind::LocalRuntime,
        model_protocol: crate::runtime::ModelProtocol::MessagesV1,
        tool_call_mode: crate::runtime::ToolCallMode::TaggedFallback,
        model_profile: ModelProfile::default_for_backend(
            crate::runtime::ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        model_headers: reqwest::header::HeaderMap::new(),
        notes_path: None,
        hooks: Vec::new(),
    };

    let (runtime, _ctx) =
        build_runtime_with_resume(config, state).expect("build_runtime_with_resume should succeed");

    assert_eq!(runtime.mode.current_task.id, "task-startup-resume");
    assert_eq!(
        runtime
            .mode
            .current_task
            .active_grants
            .get(&Capability::Network),
        Some(&ApprovalScope::Session)
    );
    assert!(
        runtime
            .mode
            .history_lines()
            .iter()
            .any(|l| l.contains("[resumed: task-startup-resume status=Running]")),
        "expected resume banner in history"
    );
}

// -- PC-01: /model --------------------------------------------------------

#[tokio::test]
async fn test_model_shows_current_name() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    mode.on_user_input("/model".to_string(), &mut ctx);
    assert!(
        mode.history_lines().iter().any(|l| l.contains("[model]")),
        "bare /model must echo current model"
    );
}

#[tokio::test]
async fn test_model_switches_name() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let old = mode.model_name.clone();
    mode.on_user_input("/model local/coder-8b".to_string(), &mut ctx);
    assert_eq!(mode.model_name, "local/coder-8b");
    assert_eq!(ctx.test_model_name().await, "local/coder-8b");
    assert!(mode
        .history_lines()
        .iter()
        .any(|l| l.contains(&old) && l.contains("local/coder-8b")));
}

#[tokio::test]
async fn test_model_rejects_local_on_api_backend() {
    let mut ctx = setup_ctx();
    let mut config = Config::default_for_tui();
    config.model_backend = crate::runtime::ModelBackendKind::ApiServer;
    config.model_name = "remote-model".to_string();
    let mut mode = TuiMode::new_with_config(None, config);
    // local/ prefix on an ApiServer session must be rejected.
    mode.on_user_input("/model local/phi-3".to_string(), &mut ctx);
    assert_ne!(
        mode.model_name, "local/phi-3",
        "must reject local/ model on api-server backend"
    );
    assert!(mode.history_lines().iter().any(|l| l.contains("rejected")));
    assert_eq!(ctx.test_model_name().await, "mock-model");
}

#[tokio::test]
async fn test_model_rejects_remote_on_local_backend() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let original = mode.model_name.clone();
    mode.on_user_input("/model remote-model".to_string(), &mut ctx);
    assert_eq!(mode.model_name, original);
    assert_eq!(ctx.test_model_name().await, "mock-model");
    assert!(mode.history_lines().iter().any(|l| l.contains("rejected")));
}

#[tokio::test]
async fn test_model_does_not_start_turn() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    let initial_messages = ctx.test_message_count().await;

    mode.on_user_input("/model".to_string(), &mut ctx);
    assert!(!mode.is_turn_in_progress(), "/model must not start a turn");

    mode.on_user_input("/model local/phi-3".to_string(), &mut ctx);
    assert!(
        !mode.is_turn_in_progress(),
        "/model <n> must not start a turn"
    );
    assert_eq!(ctx.test_message_count().await, initial_messages);
}

// -- PK-07: /diff ---------------------------------------------------------

#[tokio::test]
async fn test_tui_diff_renders_working_tree_diff() {
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("a.txt"), "hello\n").unwrap();
    git_success(temp.path(), &["add", "a.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);
    std::fs::write(temp.path().join("a.txt"), "world\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/diff".to_string(), &mut ctx);

    let has_diff = mode
        .history_lines()
        .iter()
        .any(|l| l.contains("diff --git") || l.contains("a.txt"));
    assert!(has_diff, "expected git diff output in history");
}

#[tokio::test]
async fn test_tui_diff_staged_flag() {
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("tracked.txt"), "base\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);

    std::fs::write(temp.path().join("tracked.txt"), "staged\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    std::fs::write(temp.path().join("tracked.txt"), "unstaged\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/diff --staged".to_string(), &mut ctx);

    let history = mode.history_lines().join("\n");
    assert!(history.contains("tracked.txt"));
    assert!(history.contains("+staged"));
    assert!(!history.contains("+unstaged"));
}

#[tokio::test]
async fn test_tui_diff_non_git_repo() {
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/diff".to_string(), &mut ctx);

    assert!(mode
        .history_lines()
        .iter()
        .any(|l| l == "[diff] not a git repository"));
}

#[tokio::test]
async fn test_tui_diff_clean_working_tree() {
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("clean.txt"), "clean\n").unwrap();
    git_success(temp.path(), &["add", "clean.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/diff".to_string(), &mut ctx);

    assert!(mode
        .history_lines()
        .iter()
        .any(|l| l == "[diff] working tree is clean"));
}

#[tokio::test]
async fn test_tui_diff_truncates_at_max_lines() {
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    let path = temp.path().join("large.txt");
    std::fs::write(&path, "seed\n").unwrap();
    git_success(temp.path(), &["add", "large.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);

    let large_body = (0..260)
        .map(|index| format!("line-{index}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&path, large_body).unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/diff".to_string(), &mut ctx);

    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line == "[diff truncated \u{2014} showing first 200 lines]"));
}

#[tokio::test]
async fn test_tui_diff_does_not_start_model_turn() {
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("tracked.txt"), "clean\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let initial_messages = ctx.test_message_count().await;
    mode.on_user_input("/diff".to_string(), &mut ctx);

    assert!(
        !mode.is_turn_in_progress(),
        "/diff must not start a model turn"
    );
    assert_eq!(ctx.test_message_count().await, initial_messages);
}

fn init_git_repo(path: &std::path::Path) {
    git_success(path, &["init"]);
    git_success(path, &["config", "user.name", "test"]);
    git_success(path, &["config", "user.email", "t@t"]);
}

fn git_success(path: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: stdout={} stderr={}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_tui_edit_command_starts_edit_loop() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/edit fix the parser bug".to_string(), &mut ctx);
    assert!(
        mode.active_edit_loop.is_some(),
        "/edit must set active_edit_loop"
    );
    assert!(
        mode.is_turn_in_progress(),
        "/edit must mark turn_in_progress"
    );
}

#[test]
fn test_tui_edit_command_preserves_prior_history_line() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.history_state
        .lines
        .push("prior assistant line".to_string());

    mode.on_user_input("/edit fix the parser bug".to_string(), &mut ctx);
    mode.on_model_update(UiUpdate::StreamDelta("new output".to_string()), &mut ctx);

    assert_eq!(mode.history_state.lines[0], "prior assistant line");
    assert!(
        mode.history_state
            .lines
            .iter()
            .any(|line| line.contains("new output")),
        "stream output must target the fresh placeholder line"
    );
}

#[test]
fn test_tui_fix_without_prior_loop_emits_guidance() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/fix".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[no recent validation failure in this session")),
        "expected guidance message when no prior loop exists"
    );
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_tui_fix_during_active_edit_emits_reentrancy_guard() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.active_edit_loop = Some(EditLoop::new("task-existing".to_string()));
    mode.history_state.turn_in_progress = true;
    mode.on_user_input("/fix".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[edit loop already active")),
        "expected reentrancy guard message"
    );
}

#[test]
fn test_tui_second_edit_command_blocked_while_loop_active() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.active_edit_loop = Some(EditLoop::new("task-existing".to_string()));
    mode.history_state.turn_in_progress = true;
    mode.on_user_input("/edit add more tests".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[edit loop already active")),
        "second /edit while loop active must emit reentrancy guard"
    );
}

#[test]
fn test_slash_command_returns_none_for_non_slash_input() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("hello world".to_string(), &mut ctx);
    assert!(
        mode.is_turn_in_progress(),
        "non-slash input must dispatch a model turn"
    );
}

#[test]
fn test_slash_command_does_not_call_start_turn_directly() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/edit refactor the parser".to_string(), &mut ctx);
    assert_eq!(
        mode.last_turn_input.as_deref(),
        Some("refactor the parser"),
        "/edit must pass bare instruction (not the full slash command) to start_turn"
    );
}

#[test]
fn test_tui_edit_empty_instruction_emits_usage() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/edit".to_string(), &mut ctx);
    assert!(
        mode.history_lines()
            .iter()
            .any(|l| l.contains("[edit] usage: /edit <instruction>")),
        "expected usage hint when /edit called without instruction"
    );
    assert!(!mode.is_turn_in_progress());
    assert!(mode.active_edit_loop.is_none());
}

#[test]
fn test_tui_edit_loop_completion_clears_busy_state() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/edit refactor the parser".to_string(), &mut ctx);

    mode.on_model_update(
        UiUpdate::EditLoopComplete {
            outcome: EditLoopOutcome::MaxTurnsReached { last_error: None },
            last_validation_result: None,
        },
        &mut ctx,
    );

    assert!(!mode.is_turn_in_progress());
    assert!(mode.history_state.active_assistant_index.is_none());
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("[edit loop reached max turns]")),
        "expected loop completion summary"
    );
}

#[test]
fn test_tui_new_clears_active_edit_loop_field() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_STATE_DIR", temp.path().as_os_str());

    let mut mode = TuiMode::new();
    mode.active_edit_loop = Some(EditLoop::new("task-before-new".to_string()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/new".to_string(), &mut ctx);

    assert!(
        mode.active_edit_loop.is_none(),
        "/new must clear active_edit_loop field"
    );
    std::env::remove_var("VEX_STATE_DIR");
}

#[test]
fn test_tui_clear_clears_active_edit_loop_field() {
    let mut mode = TuiMode::new();
    mode.active_edit_loop = Some(EditLoop::new("task-before-clear".to_string()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/clear".to_string(), &mut ctx);

    assert!(
        mode.active_edit_loop.is_none(),
        "/clear must clear active_edit_loop field"
    );
}

async fn wait_for_model_turn(ctx: &RuntimeContext, label: &str) {
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if ctx.test_message_count().await > 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{label} must start a single model turn"));
}

#[tokio::test]
async fn test_tui_explain_does_not_invoke_edit_loop() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx_with_responses(vec![vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"Explained\"},\"finish_reason\":\"stop\"}]}"
            .to_string(),
    ]]);

    mode.on_user_input("/explain src/app.rs".to_string(), &mut ctx);

    wait_for_model_turn(&ctx, "/explain").await;

    assert!(
        mode.active_edit_loop.is_none(),
        "/explain must not invoke EditLoop"
    );
    assert!(
        mode.last_turn_input.as_deref().is_some_and(|prompt| {
            prompt.contains("Explain the relevant code for the request below.")
                && prompt.contains("Request:\nexplain src/app.rs")
        }),
        "/explain must render the explain template prompt"
    );
}

#[tokio::test]
async fn test_tui_explain_silently_denies_tool_approval_requests() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/explain src/app.rs".to_string(), &mut ctx);

    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "apply_patch".to_string(),
            input_preview: "{\"path\":\"src/app.rs\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    assert!(
        !response_rx.await.expect("response should resolve"),
        "/explain must silently deny approval-requiring tool calls"
    );
    assert!(
        mode.overlay_state.pending_approval.is_none(),
        "/explain must not surface the approval overlay"
    );
    assert!(
        mode.history_lines()
            .iter()
            .all(|line| !line.contains("[tool approval requested:")),
        "/explain denial should stay silent in transcript output"
    );
}

#[tokio::test]
async fn test_read_only_turn_flag_clears_after_turn_completion() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/explain src/app.rs".to_string(), &mut ctx);
    assert!(
        mode.read_only_turn_active,
        "/explain must mark the active turn as read-only"
    );

    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);
    assert!(
        !mode.read_only_turn_active,
        "turn completion must clear the read-only turn flag"
    );

    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "apply_patch".to_string(),
            input_preview: "{\"path\":\"src/app.rs\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    let mut response_rx = Box::pin(response_rx);
    assert!(
        response_rx.as_mut().now_or_never().is_none(),
        "normal turns must keep approval unresolved until operator input"
    );
    assert!(
        mode.overlay_state.pending_approval.is_some(),
        "normal turns must restore the approval overlay"
    );
}

#[tokio::test]
async fn test_tui_review_default_assembles_head_diff() {
    let mut ctx = setup_ctx_with_responses(vec![vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"Reviewed\"},\"finish_reason\":\"stop\"}]}"
            .to_string(),
    ]]);
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("tracked.txt"), "hello\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);
    std::fs::write(temp.path().join("tracked.txt"), "world\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/review".to_string(), &mut ctx);

    wait_for_model_turn(&ctx, "/review").await;

    assert!(
        mode.active_edit_loop.is_none(),
        "/review must not invoke EditLoop"
    );
    assert!(
        mode.last_turn_input.as_deref().is_some_and(|prompt| {
            prompt.contains("Review the implementation described below.")
                && prompt.contains(
                    "Review these changes for correctness, clarity, and potential issues.",
                )
                && prompt.contains("Diff context:\n")
                && prompt.contains("diff --git")
                && prompt.contains("tracked.txt")
        }),
        "/review must render the review prompt with git diff context"
    );
}

#[tokio::test]
async fn test_tui_review_base_flag_validates_ref() {
    let mut ctx = setup_ctx_with_responses(vec![vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"Reviewed\"},\"finish_reason\":\"stop\"}]}"
            .to_string(),
    ]]);
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("tracked.txt"), "base\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);
    std::fs::write(temp.path().join("tracked.txt"), "changed\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    git_success(temp.path(), &["commit", "-m", "change"]);

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/review --base HEAD~1 inspect".to_string(), &mut ctx);

    wait_for_model_turn(&ctx, "/review --base").await;

    assert!(
        mode.last_turn_input.as_deref().is_some_and(|prompt| {
            prompt.contains("Request:\ninspect")
                && prompt.contains("diff --git")
                && prompt.contains("tracked.txt")
                && prompt.contains("+changed")
        }),
        "/review --base must start a turn with the requested diff"
    );
}

#[tokio::test]
async fn test_tui_review_invalid_ref_emits_error_no_turn() {
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("tracked.txt"), "base\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let initial_messages = ctx.test_message_count().await;
    mode.on_user_input("/review --base missing-ref".to_string(), &mut ctx);

    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line == "[review: invalid base ref 'missing-ref']"));
    assert!(
        !mode.is_turn_in_progress(),
        "invalid /review base refs must not start a turn"
    );
    assert_eq!(ctx.test_message_count().await, initial_messages);
}

#[tokio::test]
async fn test_tui_review_mutual_exclusion_base_and_files() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    let initial_messages = ctx.test_message_count().await;

    mode.on_user_input(
        "/review --base HEAD --files src/*.rs inspect".to_string(),
        &mut ctx,
    );

    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line == "[review: --base and --files are mutually exclusive]"));
    assert!(!mode.is_turn_in_progress());
    assert_eq!(ctx.test_message_count().await, initial_messages);
}

#[tokio::test]
async fn test_tui_review_drops_pending_patch_silently() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("tracked.txt"), "hello\n").unwrap();
    git_success(temp.path(), &["add", "tracked.txt"]);
    git_success(temp.path(), &["commit", "-m", "init"]);
    std::fs::write(temp.path().join("tracked.txt"), "world\n").unwrap();

    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/review".to_string(), &mut ctx);

    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<bool>();
    mode.on_model_update(
        UiUpdate::ToolApprovalRequest(ToolApprovalRequest {
            tool_name: "apply_patch".to_string(),
            input_preview: "{\"path\":\"tracked.txt\"}".to_string(),
            response_tx,
        }),
        &mut ctx,
    );

    assert!(
        !response_rx.await.expect("response should resolve"),
        "/review must silently deny approval-requiring tool calls"
    );
    assert!(
        mode.overlay_state.pending_approval.is_none(),
        "/review must not surface the approval overlay"
    );
    assert!(
        mode.history_lines()
            .iter()
            .all(|line| !line.contains("[tool approval requested:")),
        "/review denial should stay silent in transcript output"
    );
}

#[tokio::test]
async fn test_tui_review_files_flag_uses_context_assembler() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx_with_responses(vec![vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"Reviewed\"},\"finish_reason\":\"stop\"}]}"
            .to_string(),
    ]]);
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();

    mode.working_dir = temp.path().to_path_buf();
    mode.on_user_input("/review --files src/*.rs inspect".to_string(), &mut ctx);

    wait_for_model_turn(&ctx, "/review --files").await;

    let assembled = mode
        .last_assembled_context
        .as_ref()
        .expect("/review --files must capture assembled context");
    assert!(assembled
        .file_snapshots
        .iter()
        .any(|snapshot| snapshot.path == std::path::Path::new("src/lib.rs")));
    assert!(
        mode.last_turn_input.as_deref().is_some_and(|prompt| {
            prompt.contains("[review files] pattern: src/*.rs")
                && prompt.contains("src/lib.rs")
                && prompt.contains("pub fn answer() -> i32 { 42 }")
        }),
        "/review --files must render assembled file context"
    );
}

#[test]
fn test_tui_run_command_invokes_validation_suite_only() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input(successful_run_input(), &mut ctx);

    assert!(
        !mode.is_turn_in_progress(),
        "/run must not start a model turn"
    );
    assert!(
        mode.active_edit_loop.is_none(),
        "/run must not seed or invoke EditLoop"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("[run]")),
        "expected /run transcript output"
    );
}

#[test]
fn test_at_path_injects_file_contents() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("note.txt"), "hello from file\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input("summarize @note.txt".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("[file: note.txt]"));
    assert!(turn_input.contains("hello from file"));
    assert!(mode.is_turn_in_progress());
}

#[test]
fn test_at_path_directory_renders_listing() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("src")).unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), "pub fn hi() {}\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input("review @src".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("[dir: src]"));
    assert!(turn_input.contains("src/lib.rs"));
}

#[test]
fn test_at_path_missing_file_is_annotated() {
    let temp = tempfile::tempdir().unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input("inspect @missing.txt".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("[file: missing.txt \u{2014} not found]"));
}

#[test]
fn test_at_path_outside_workspace_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_path = outside.path().join("secret.txt");
    std::fs::write(&outside_path, "secret").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input(
        format!("inspect @{}", outside_path.to_string_lossy()),
        &mut ctx,
    );

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("[file: "));
    assert!(
        turn_input.contains("outside workspace root")
            || turn_input.contains("escapes workspace root")
            || turn_input.contains("absolute or platform-specific path not allowed"),
        "expected outside-workspace annotation, got: {turn_input}"
    );
}

#[test]
fn test_at_path_multiple_tokens_resolved_in_order() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("one.txt"), "first").unwrap();
    std::fs::write(temp.path().join("two.txt"), "second").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input("compare @one.txt with @two.txt".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    let first_idx = turn_input.find("[file: one.txt]").unwrap();
    let second_idx = turn_input.find("[file: two.txt]").unwrap();
    assert!(first_idx < second_idx);
    assert!(turn_input.contains("first"));
    assert!(turn_input.contains("second"));
}

#[test]
fn test_at_path_not_expanded_inside_slash_command_args() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("note.txt"), "hello from file\n").unwrap();

    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input("/explain @note.txt".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(!turn_input.contains("[file: note.txt]"));
    assert!(!turn_input.contains("hello from file"));
}

#[test]
fn test_bang_prefix_requires_run_command_approval() {
    let temp = tempfile::tempdir().unwrap();
    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let mut ctx = setup_ctx();

    mode.on_user_input(successful_bang_input(), &mut ctx);

    assert!(mode.overlay_state.pending_approval.is_some());
    assert!(!mode.is_turn_in_progress());
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| { line.contains("[tool approval requested:") }));
}

#[tokio::test]
async fn test_bang_prefix_runs_without_model_turn_after_approval() {
    let temp = tempfile::tempdir().unwrap();
    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let (mut ctx, mut rx) = setup_ctx_with_updates();
    let initial_messages = ctx.test_message_count().await;

    mode.on_user_input(successful_bang_input(), &mut ctx);
    assert!(mode.overlay_state.pending_approval.is_some());
    assert!(!mode.is_turn_in_progress());

    mode.on_user_input("1".to_string(), &mut ctx);
    assert!(mode.is_turn_in_progress());

    drain_until_turn_complete(&mut mode, &mut ctx, &mut rx).await;

    assert!(mode.overlay_state.pending_approval.is_none());
    assert!(
        !mode.command_session_active(),
        "command session completion should restore normal TUI polling"
    );
    assert!(!mode.is_turn_in_progress());
    assert_eq!(ctx.test_message_count().await, initial_messages);
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("[command session started")),
        "expected command session start marker in transcript"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("inline-shell")),
        "expected captured shell output in transcript"
    );
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[command session exit: 0]"),
        "expected command session exit status"
    );
}

#[tokio::test]
async fn test_shell_command_runner_invokes_sandbox_wrap() {
    let temp = tempfile::tempdir().unwrap();
    let wrapped = Arc::new(AtomicBool::new(false));
    let result = run_shell_command_with_runner(
        DefaultCommandRunner::new(),
        RecordingSandbox {
            wrapped: Arc::clone(&wrapped),
        },
        "echo sandbox-hit".to_string(),
        temp.path().to_path_buf(),
    )
    .await
    .unwrap();

    assert!(wrapped.load(Ordering::SeqCst));
    assert!(result.stdout.contains("sandbox-hit"));
}

#[tokio::test]
async fn test_shell_command_request_invokes_sandbox_wrap() {
    let temp = tempfile::tempdir().unwrap();
    let wrapped = Arc::new(AtomicBool::new(false));
    let result = run_shell_command_with_runner(
        DefaultCommandRunner::new(),
        RecordingSandbox {
            wrapped: Arc::clone(&wrapped),
        },
        "echo passthrough-hit".to_string(),
        temp.path().to_path_buf(),
    )
    .await
    .unwrap();

    assert!(wrapped.load(Ordering::SeqCst));
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("passthrough-hit"));
}

#[test]
fn test_command_session_updates_track_matching_session() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.history_state.turn_in_progress = true;
    mode.begin_turn_capture("!first".to_string());
    let first = mode.begin_command_session("first".to_string());
    let second = mode.begin_command_session("second".to_string());

    mode.on_model_update(
        UiUpdate::CommandSessionAttached {
            session_id: second,
            pid: Some(22),
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::CommandSessionAttached {
            session_id: first,
            pid: Some(11),
        },
        &mut ctx,
    );

    assert_eq!(mode.command_sessions[0].pid, Some(11));
    assert_eq!(mode.command_sessions[1].pid, Some(22));

    mode.on_model_update(
        UiUpdate::CommandSessionFinished { session_id: first },
        &mut ctx,
    );
    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

    assert_eq!(mode.command_sessions.len(), 1);
    assert!(mode.is_turn_in_progress());
    assert_eq!(mode.command_sessions[0].command, "second");

    mode.on_model_update(
        UiUpdate::CommandSessionFinished { session_id: second },
        &mut ctx,
    );
    mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

    assert!(mode.command_sessions.is_empty());
    assert!(!mode.is_turn_in_progress());
}

#[test]
fn test_command_session_started_update_creates_running_session() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.history_state.turn_in_progress = true;
    mode.on_model_update(
        UiUpdate::CommandSessionStarted {
            session_id: 77,
            command: "echo from-tool".to_string(),
        },
        &mut ctx,
    );
    mode.on_model_update(
        UiUpdate::CommandSessionAttached {
            session_id: 77,
            pid: Some(7700),
        },
        &mut ctx,
    );

    assert_eq!(mode.command_sessions.len(), 1);
    assert_eq!(mode.command_sessions[0].id, 77);
    assert_eq!(mode.command_sessions[0].command, "echo from-tool");
    assert_eq!(mode.command_sessions[0].pid, Some(7700));
    assert_eq!(mode.command_sessions[0].status, "running");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_model_run_command_streams_managed_session_into_tui_transcript() {
    let temp = tempfile::tempdir().unwrap();
    let responses = vec![
            vec![
                r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_run_command_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
                r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
                r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Running a command now."}}"#.to_string(),
                r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_run_command_01","name":"run_command","input":{}}}"#.to_string(),
                #[cfg(windows)]
                r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"command\":\"cmd\",\"args\":[\"/C\",\"echo model-tool-output\"]}"}}"#.to_string(),
                #[cfg(not(windows))]
                r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"command\":\"sh\",\"args\":[\"-c\",\"printf 'model-tool-output\\n'\"]}"}}"#.to_string(),
                r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#.to_string(),
                r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":6}}"#.to_string(),
                r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
            ],
            vec![
                r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_run_command_02","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
                r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
                r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Finished running the command."}}"#.to_string(),
                r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":8}}"#.to_string(),
                r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
            ],
        ];

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    mode.current_task
        .active_grants
        .insert(Capability::RunCommand, ApprovalScope::Session);
    let (mut ctx, mut rx) = setup_ctx_with_responses_and_updates(responses);

    mode.on_user_input("run the managed tool command".to_string(), &mut ctx);
    drain_until_turn_complete(&mut mode, &mut ctx, &mut rx).await;

    let lines = mode.history_lines();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("[command session started")),
        "expected managed command-session start marker in transcript"
    );
    assert!(
        lines.iter().any(|line| line.contains("model-tool-output")),
        "expected model run_command output in transcript"
    );
    assert!(
        lines.iter().any(|line| line == "[command session exit: 0]"),
        "expected managed command-session exit marker in transcript"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Finished running the command.")),
        "expected final assistant response after tool completion"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_bang_prefix_cancellation_completes_turn() {
    let temp = tempfile::tempdir().unwrap();
    let mut mode = TuiMode::new();
    mode.working_dir = temp.path().to_path_buf();
    let (mut ctx, mut rx) = setup_ctx_with_updates();
    let input = if cfg!(windows) {
        "!ping -n 60 127.0.0.1 > nul".to_string()
    } else {
        "!sleep 30".to_string()
    };

    mode.on_user_input(input, &mut ctx);
    mode.on_user_input("1".to_string(), &mut ctx);
    assert!(mode.is_turn_in_progress());

    mode.on_interrupt(&mut ctx);
    drain_until_turn_complete(&mut mode, &mut ctx, &mut rx).await;

    assert!(!mode.is_turn_in_progress());
    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line == "[command session cancelled]"),
        "expected cancellation feedback for command sessions"
    );
}

#[tokio::test]
async fn test_tui_context_renders_without_model_turn() {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    let initial_messages = ctx.test_message_count().await;

    mode.on_user_input("/context".to_string(), &mut ctx);

    assert!(
        !mode.is_turn_in_progress(),
        "/context must not start a model turn"
    );
    assert_eq!(
        ctx.test_message_count().await,
        initial_messages,
        "/context must not call ctx.start_turn"
    );
    assert!(
        mode.history_lines().iter().any(|line| line == "[context]"),
        "expected context header"
    );
}

#[test]
fn test_tui_context_shows_tilde_token_estimate() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/context".to_string(), &mut ctx);

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.trim_start().starts_with("tokens") && line.contains('~')),
        "token estimate line must include '~'"
    );
}

#[test]
fn test_tui_context_shows_active_grants_count() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.current_task
        .active_grants
        .insert(Capability::RunCommand, ApprovalScope::Session);

    mode.on_user_input("/context".to_string(), &mut ctx);

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("1 active grant(s)")),
        "expected active grants count in /context output"
    );
}

#[test]
fn test_tui_context_shows_active_profile_name() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    let mut profile =
        ModelProfile::default_for_backend(crate::runtime::ModelBackendKind::ApiServer);
    profile.name = "api-structured".to_string();
    mode.active_edit_loop = Some(EditLoop::new("task-profile".to_string()).with_profile(profile));

    mode.on_user_input("/context".to_string(), &mut ctx);

    assert!(
        mode.history_lines()
            .iter()
            .any(|line| line.contains("profile") && line.contains("api-structured")),
        "expected active profile name in /context output"
    );
}

#[test]
fn test_tui_commands_renders_all_registered_commands() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/commands".to_string(), &mut ctx);

    assert!(
        mode.history_lines().iter().any(|line| line == "[commands]"),
        "expected commands header"
    );
    for spec in SLASH_COMMANDS {
        assert!(
            mode.history_lines()
                .iter()
                .any(|line| line.contains(spec.display) && line.contains(spec.description)),
            "expected '{}' to appear in /commands output",
            spec.display
        );
    }
}

#[test]
fn test_tui_help_is_alias_for_commands() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut commands_mode = TuiMode::new();
    let mut help_mode = TuiMode::new();
    let mut ctx = setup_ctx();

    commands_mode.on_user_input("/commands".to_string(), &mut ctx);
    help_mode.on_user_input("/help".to_string(), &mut ctx);

    assert_eq!(
        &commands_mode.history_lines()[2..],
        &help_mode.history_lines()[2..],
        "/help must render the same command directory as /commands"
    );
}

#[tokio::test]
async fn test_commands_output_does_not_call_start_turn() {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    let initial_messages = ctx.test_message_count().await;

    mode.on_user_input("/commands".to_string(), &mut ctx);

    assert!(
        !mode.is_turn_in_progress(),
        "/commands must not start a model turn"
    );
    assert_eq!(
        ctx.test_message_count().await,
        initial_messages,
        "/commands must not call ctx.start_turn"
    );
}

#[test]
fn test_custom_command_appears_in_commands_list() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    write_custom_command(
        &temp.path().join(".vex/commands"),
        "standup.toml",
        "standup",
        "summarise changes",
        "Summarise {{input}} using {{context}}",
    );

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/commands".to_string(), &mut ctx);

    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line == "[custom commands]"));
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line.contains("/standup [input]") && line.contains("summarise changes")));
}

#[test]
fn test_custom_command_invokes_single_turn() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    write_custom_command(
        &temp.path().join(".vex/commands"),
        "standup.toml",
        "standup",
        "summarise changes",
        "Prompt: {{input}}",
    );

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/standup src/lib.rs".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(mode.is_turn_in_progress());
    assert!(turn_input.contains("Prompt: src/lib.rs"));
}

#[test]
fn test_custom_command_context_substitution() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
    write_custom_command(
        &temp.path().join(".vex/commands"),
        "standup.toml",
        "standup",
        "summarise changes",
        "Context:\n{{context}}",
    );

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/standup src/lib.rs".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("pub fn answer() -> i32 { 42 }"));
    assert_eq!(
        turn_input.matches("pub fn answer() -> i32 { 42 }").count(),
        1,
        "custom command context must be injected exactly once"
    );
    assert_eq!(
        turn_input.matches("## Context").count(),
        1,
        "custom command context header must not be duplicated"
    );
}

#[test]
fn test_custom_command_project_scoped_takes_precedence() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let xdg = tempfile::tempdir().unwrap();
    std::env::set_var("XDG_CONFIG_HOME", xdg.path());

    write_custom_command(
        &xdg.path().join("vex/commands"),
        "standup.toml",
        "standup",
        "user",
        "USER TEMPLATE",
    );
    write_custom_command(
        &temp.path().join(".vex/commands"),
        "standup.toml",
        "standup",
        "project",
        "PROJECT TEMPLATE",
    );

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/standup".to_string(), &mut ctx);
    std::env::remove_var("XDG_CONFIG_HOME");

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("PROJECT TEMPLATE"));
    assert!(!turn_input.contains("USER TEMPLATE"));
}

#[test]
fn test_custom_command_cannot_shadow_builtin() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    write_custom_command(
        &temp.path().join(".vex/commands"),
        "edit.toml",
        "edit",
        "shadow builtin",
        "shadow",
    );

    let mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    assert!(mode.custom_commands.is_empty());
}

#[test]
fn test_tui_tools_renders_builtin_tools() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/tools".to_string(), &mut ctx);

    assert!(mode.history_lines().iter().any(|line| line == "[tools]"));
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line.contains("built-in tools only")));
    for tool in builtin_tool_summaries() {
        assert!(
            mode.history_lines()
                .iter()
                .any(|line| line.trim() == tool.name),
            "expected '{}' in /tools output",
            tool.name
        );
    }
}

#[test]
fn test_tui_tools_desc_includes_descriptions() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();

    mode.on_user_input("/tools desc".to_string(), &mut ctx);

    for tool in builtin_tool_summaries() {
        assert!(
            mode.history_lines()
                .iter()
                .any(|line| line.contains(&tool.name) && line.contains(&tool.description)),
            "expected '{}' description in /tools desc output",
            tool.name
        );
    }
}

#[test]
fn test_usage_command_uses_last_turn_estimate_flag() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    ctx.test_record_session_turn(crate::usage::TurnTokens {
        input: 10,
        output: 5,
        estimated: true,
    });
    ctx.test_record_session_turn(crate::usage::TurnTokens {
        input: 4,
        output: 3,
        estimated: false,
    });

    mode.on_user_input("/usage".to_string(), &mut ctx);

    assert!(mode.history_lines().iter().any(|line| line == "[usage]"));
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line == "  this turn   : 4 in / 3 out"));
    assert!(mode
        .history_lines()
        .iter()
        .any(|line| line == "  session     : 14 in / 8 out (estimated)"));
}

#[tokio::test]
async fn test_tui_tools_does_not_start_model_turn() {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    let initial_messages = ctx.test_message_count().await;

    mode.on_user_input("/tools".to_string(), &mut ctx);

    assert!(!mode.is_turn_in_progress());
    assert_eq!(ctx.test_message_count().await, initial_messages);
}

#[test]
fn test_tui_generate_tests_assembles_context() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/generate-tests src/lib.rs".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("generate tests for src/lib.rs"));
    assert!(turn_input.contains("pub fn answer() -> i32 { 42 }"));
}

#[test]
fn test_tui_generate_tests_infers_framework() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\n",
    )
    .unwrap();

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();
    mode.on_user_input("/generate-tests src/lib.rs".to_string(), &mut ctx);

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("Preferred test framework: cargo-test"));
}

#[test]
fn test_tui_generate_tests_framework_flag_overrides_inference() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\n",
    )
    .unwrap();

    let mut mode = TuiMode::new_with_config(None, config_with_workdir(temp.path()));
    let mut ctx = setup_ctx();
    mode.on_user_input(
        "/generate-tests src/lib.rs --framework jest".to_string(),
        &mut ctx,
    );

    let turn_input = mode.last_turn_input.as_deref().unwrap_or_default();
    assert!(turn_input.contains("Preferred test framework: jest"));
    assert!(!turn_input.contains("Preferred test framework: cargo-test"));
}

#[test]
fn test_missing_command_description_is_compile_error() {
    assert!(
        !SLASH_COMMANDS.is_empty(),
        "slash command registry must not be empty"
    );
    for spec in SLASH_COMMANDS {
        assert!(
            !spec.description.is_empty(),
            "command '{}' must have a description",
            spec.display
        );
    }
}
