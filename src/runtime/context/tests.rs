use super::{RuntimeContext, estimate_token_count, forward_conversation_update};
use crate::api::{ApiClient, mock_client::MockApiClient};
use crate::prompts::CODER_SYSTEM_PROMPT;
use crate::runtime::{EditLoop, EditLoopOutcome, UiUpdate};
use crate::state::{ConversationManager, ConversationStreamUpdate, StreamBlock};
use crate::types::{ApiMessage, Content, ContentBlock};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_ref_04_start_turn_dispatches_message() {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();

    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n".to_string(),
        "data: {\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}]}\n\n"
            .to_string(),
    ]])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());

    let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

    ctx.start_pulse("test input".to_string());

    let mut saw_delta = false;
    let mut saw_complete = false;
    loop {
        match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(UiUpdate::StreamDelta(_))) | Ok(Some(UiUpdate::StreamBlockDelta { .. })) => {
                saw_delta = true
            }
            Ok(Some(UiUpdate::PulseComplete)) => {
                saw_complete = true;
                break;
            }
            Ok(Some(UiUpdate::Error(e))) => panic!("unexpected error: {e}"),
            Ok(None) | Err(_) => break,
            _ => {}
        }
    }

    assert!(saw_delta, "expected at least one streamed text update");
    assert!(saw_complete, "expected PulseComplete");
}

#[test]
fn test_ref_07_no_runtime_guard() {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

    ctx.start_pulse("test".to_string());

    let update = rx.try_recv().expect("expected error update");
    match update {
        UiUpdate::Error(msg) => {
            assert!(
                msg.contains("requires active Tokio runtime"),
                "unexpected error message: {msg}"
            );
        }
        _ => panic!("expected UiUpdate::Error, got something else"),
    }

    assert_eq!(
        ctx.test_message_count_try_lock(),
        Some(0),
        "history must stay clean when guard fires"
    );
}

#[tokio::test]
async fn test_start_edit_loop_dispatches_loop_completion() {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

    ctx.start_edit_loop(
        EditLoop::new("task-edit-loop".to_string()).with_max_turns(1),
        "fix the parser".to_string(),
    );

    loop {
        match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(UiUpdate::TranscriptLine(_))) => {}
            Ok(Some(UiUpdate::EditLoopComplete { outcome, .. })) => {
                assert!(matches!(outcome, EditLoopOutcome::MaxTurnsReached { .. }));
                return;
            }
            Ok(Some(UiUpdate::Error(e))) => panic!("unexpected error: {e}"),
            Ok(None) | Err(_) => panic!("expected EditLoopComplete"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn test_coding_prompt_injected_during_edit_loop_only() {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

    assert!(
        !ctx.test_system_prompt()
            .await
            .contains(CODER_SYSTEM_PROMPT.trim()),
        "base prompt must not include the coding prompt before edit loop activation"
    );

    ctx.start_edit_loop(
        EditLoop::new("task-edit-loop-prompt".to_string())
            .with_max_turns(128)
            .with_working_dir(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
        "fix the parser".to_string(),
    );

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if ctx
                .test_system_prompt()
                .await
                .contains(CODER_SYSTEM_PROMPT.trim())
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("coding prompt must be injected while the edit loop is active");

    loop {
        match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
            Ok(Some(UiUpdate::EditLoopComplete { .. })) => break,
            Ok(Some(UiUpdate::TranscriptLine(_))) => {}
            Ok(Some(UiUpdate::Error(e))) => panic!("unexpected error: {e}"),
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => panic!("expected EditLoopComplete"),
        }
    }

    assert!(
        !ctx.test_system_prompt()
            .await
            .contains(CODER_SYSTEM_PROMPT.trim()),
        "coding prompt must clear after the edit loop completes"
    );
}

#[test]
fn test_estimated_token_count_uses_chars_div_4() {
    let messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("abcd".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Blocks(vec![
                ContentBlock::Text {
                    text: "efgh".to_string(),
                    citations: None,
                },
                ContentBlock::ToolResult {
                    tool_use_id: "tool-1".to_string(),
                    content: "ijkl".to_string(),
                    is_error: false,
                },
            ]),
            cache_hint: None,
        },
    ];

    assert_eq!(estimate_token_count(&messages), 7);
}

#[tokio::test]
async fn test_ref_08_start_turn_full_protocol_parity() {
    let chunks = vec![vec![
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n".to_string(),
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n".to_string(),
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n\n".to_string(),
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n".to_string(),
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
    ]];

    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(chunks)));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

    ctx.start_pulse("test".to_string());

    let mut events: Vec<&str> = vec![];
    loop {
        match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(UiUpdate::StreamBlockStart { .. })) => events.push("BlockStart"),
            Ok(Some(UiUpdate::StreamBlockDelta { .. })) => events.push("BlockDelta"),
            Ok(Some(UiUpdate::StreamBlockComplete { .. })) => events.push("BlockComplete"),
            Ok(Some(UiUpdate::StreamDelta(_))) => events.push("Delta"),
            Ok(Some(UiUpdate::ToolApprovalRequest(_))) => events.push("ToolApproval"),
            Ok(Some(UiUpdate::PulseComplete)) => {
                events.push("PulseComplete");
                break;
            }
            Ok(Some(UiUpdate::Error(e))) => panic!("unexpected error: {e}"),
            _ => break,
        }
    }

    assert!(
        events.contains(&"PulseComplete"),
        "must terminate with PulseComplete"
    );
    assert_eq!(
        events.iter().filter(|&&e| e == "PulseComplete").count(),
        1,
        "exactly one PulseComplete"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_ref_08_tool_approval_forwarding_no_hang() {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    crate::test_support::test_set_var(&_env_lock, "VEX_TOOL_CONFIRM", "true");
    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_tool_then_final_1","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_mock_round_1","name":"read_file","input":{}}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"file.txt\"}"}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":10}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let second_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_tool_then_final_2","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"done"}}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":7}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![
        first_response_sse,
        second_response_sse,
    ])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

    ctx.start_pulse("read file".to_string());

    let mut saw_request = false;
    let mut saw_complete = false;
    loop {
        match tokio::time::timeout(Duration::from_millis(800), rx.recv()).await {
            Ok(Some(UiUpdate::ToolApprovalRequest(request))) => {
                saw_request = true;
                let _ = request.response_tx.send(false);
            }
            Ok(Some(UiUpdate::PulseComplete)) => {
                saw_complete = true;
                break;
            }
            Ok(Some(UiUpdate::Error(e))) => panic!("unexpected error: {e}"),
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert!(saw_request, "must forward tool approval request");
    assert!(saw_complete, "must finish pulse after approval response");
    crate::test_support::test_remove_var(&_env_lock, "VEX_TOOL_CONFIRM");
}

#[tokio::test]
async fn test_ref_08_block_delta_partial_json_not_mirrored_to_stream_delta() {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut textual_block_by_index = std::collections::HashMap::new();

    let mut normaliser = crate::api::stream::StreamTextNormaliser::new();
    forward_conversation_update(
        ConversationStreamUpdate::BlockStart {
            index: 1,
            block: StreamBlock::ToolCall {
                id: "toolu_1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({}),
                status: crate::state::ToolStatus::Pending,
            },
        },
        &mut textual_block_by_index,
        &mut normaliser,
        &tx,
    );

    forward_conversation_update(
        ConversationStreamUpdate::BlockDelta {
            index: 1,
            delta: "{\"path\":\"file.txt\"}".to_string(),
        },
        &mut textual_block_by_index,
        &mut normaliser,
        &tx,
    );

    forward_conversation_update(
        ConversationStreamUpdate::ToolCallArgumentsUpdated {
            tool_call_id: "toolu_1".to_string(),
            tool_name: Some("read_file".to_string()),
            arguments: serde_json::json!({"path": "file.txt"}),
        },
        &mut textual_block_by_index,
        &mut normaliser,
        &tx,
    );

    let mut saw_block_delta = false;
    let mut saw_tool_arguments = false;
    let mut leaked_stream_delta = false;
    for _ in 0..5 {
        match rx.try_recv() {
            Ok(UiUpdate::StreamBlockDelta { delta, .. }) if delta.contains("path") => {
                saw_block_delta = true
            }
            Ok(UiUpdate::ToolCallArgumentsUpdated {
                tool_call_id,
                tool_name,
                arguments,
            }) => {
                saw_tool_arguments = tool_call_id == "toolu_1"
                    && tool_name.as_deref() == Some("read_file")
                    && arguments == serde_json::json!({"path": "file.txt"});
            }
            Ok(UiUpdate::StreamDelta(text)) if text.contains("path") => leaked_stream_delta = true,
            Ok(_) => {}
            Err(_) => break,
        }
    }

    assert!(
        saw_block_delta,
        "expected StreamBlockDelta from partial_json"
    );
    assert!(
        saw_tool_arguments,
        "expected ToolCallArgumentsUpdated when structured arguments are available"
    );
    assert!(
        !leaked_stream_delta,
        "partial_json must not leak into StreamDelta"
    );
}

#[tokio::test]
async fn test_ref_08_unknown_block_index_delta_does_not_mirror_to_stream_delta() {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut textual_block_by_index = std::collections::HashMap::new();
    let mut normaliser = crate::api::stream::StreamTextNormaliser::new();

    forward_conversation_update(
        ConversationStreamUpdate::BlockDelta {
            index: 99,
            delta: "mystery".to_string(),
        },
        &mut textual_block_by_index,
        &mut normaliser,
        &tx,
    );

    let mut saw_block_delta = false;
    let mut saw_stream_delta = false;
    while let Ok(update) = rx.try_recv() {
        match update {
            UiUpdate::StreamBlockDelta { .. } => saw_block_delta = true,
            UiUpdate::StreamDelta(_) => saw_stream_delta = true,
            _ => {}
        }
    }

    assert!(saw_block_delta, "block delta should always be forwarded");
    assert!(
        !saw_stream_delta,
        "unknown block index must not mirror into StreamDelta"
    );
}

#[tokio::test]
async fn test_ref_08_command_session_updates_forward_to_ui() {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut textual_block_by_index = std::collections::HashMap::new();
    let mut normaliser = crate::api::stream::StreamTextNormaliser::new();

    forward_conversation_update(
        ConversationStreamUpdate::CommandSessionStarted {
            session_id: 41,
            command: "echo forwarded".to_string(),
        },
        &mut textual_block_by_index,
        &mut normaliser,
        &tx,
    );
    forward_conversation_update(
        ConversationStreamUpdate::CommandSessionAttached {
            session_id: 41,
            pid: Some(4100),
        },
        &mut textual_block_by_index,
        &mut normaliser,
        &tx,
    );
    forward_conversation_update(
        ConversationStreamUpdate::TranscriptLine("forwarded".to_string()),
        &mut textual_block_by_index,
        &mut normaliser,
        &tx,
    );
    forward_conversation_update(
        ConversationStreamUpdate::CommandSessionFinished { session_id: 41 },
        &mut textual_block_by_index,
        &mut normaliser,
        &tx,
    );

    match rx.recv().await {
        Some(UiUpdate::CommandSessionStarted {
            session_id,
            command,
        }) => {
            assert_eq!(session_id, 41);
            assert_eq!(command, "echo forwarded");
        }
        _ => panic!("expected CommandSessionStarted"),
    }

    match rx.recv().await {
        Some(UiUpdate::CommandSessionAttached { session_id, pid }) => {
            assert_eq!(session_id, 41);
            assert_eq!(pid, Some(4100));
        }
        _ => panic!("expected CommandSessionAttached"),
    }

    match rx.recv().await {
        Some(UiUpdate::TranscriptLine(line)) => assert_eq!(line, "forwarded"),
        _ => panic!("expected TranscriptLine"),
    }

    match rx.recv().await {
        Some(UiUpdate::CommandSessionFinished { session_id }) => {
            assert_eq!(session_id, 41);
        }
        _ => panic!("expected CommandSessionFinished"),
    }
}

#[tokio::test]
async fn test_ref_08_cancel_turn_resets_root_token_for_next_turn() {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![vec![
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n".to_string(),
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n".to_string(),
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n".to_string(),
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
    ]])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

    assert!(!ctx.test_root_cancelled());
    ctx.cancel_pulse();
    assert!(
        !ctx.test_root_cancelled(),
        "cancel_pulse must replace root token with a fresh non-cancelled token"
    );

    ctx.start_pulse("pulse B".to_string());

    let progressed = tokio::time::timeout(Duration::from_millis(800), async {
        loop {
            match rx.recv().await {
                Some(UiUpdate::StreamDelta(_) | UiUpdate::PulseComplete) => return true,
                Some(UiUpdate::Error(_)) | None => return false,
                Some(_) => {}
            }
        }
    })
    .await
    .unwrap_or(false);

    assert!(
        progressed,
        "pulse after cancel_pulse must emit at least one normal update with fresh root token"
    );
}

#[tokio::test]
async fn test_ref_08_cancel_path_emits_single_terminal_event() {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n".to_string(),
    ]])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

    ctx.start_pulse("test".to_string());
    ctx.cancel_pulse();

    let mut terminal_count = 0;
    for _ in 0..6 {
        match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
            Ok(Some(UiUpdate::PulseComplete | UiUpdate::Error(_))) => terminal_count += 1,
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert_eq!(
        terminal_count, 1,
        "cancel path must emit exactly one final event"
    );
}

#[tokio::test]
async fn test_text_block_markup_is_forwarded_verbatim() {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut textual_block_by_index = std::collections::HashMap::new();
    let mut normaliser = crate::api::stream::StreamTextNormaliser::new();

    let content = "<function=read_file>\n<parameter=path>\nsrc/main.rs\n</parameter>\n</function>"
        .to_string();

    forward_conversation_update(
        ConversationStreamUpdate::BlockStart {
            index: 0,
            block: StreamBlock::FinalText {
                content: content.clone(),
            },
        },
        &mut textual_block_by_index,
        &mut normaliser,
        &tx,
    );

    let mut saw_block_start = false;
    let mut saw_verbatim_delta = false;
    while let Ok(update) = rx.try_recv() {
        match update {
            UiUpdate::StreamBlockStart {
                block: StreamBlock::FinalText { content },
                ..
            } => {
                saw_block_start = true;
                assert!(content.is_empty());
            }
            UiUpdate::StreamBlockDelta { delta, .. } if delta == content => {
                saw_verbatim_delta = true;
            }
            _ => {}
        }
    }

    assert!(saw_block_start, "expected block-start event");
    assert!(saw_verbatim_delta, "expected verbatim block delta");
}

#[tokio::test]
async fn test_normaliser_passes_clean_text_through_delta() {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut textual_block_by_index = std::collections::HashMap::new();
    let mut normaliser = crate::api::stream::StreamTextNormaliser::new();

    forward_conversation_update(
        ConversationStreamUpdate::Delta("Hello, world!".to_string()),
        &mut textual_block_by_index,
        &mut normaliser,
        &tx,
    );

    match rx.try_recv() {
        Ok(UiUpdate::StreamDelta(text)) => {
            assert_eq!(text, "Hello, world!");
        }
        _ => panic!("expected StreamDelta with clean text"),
    }
}

#[tokio::test]
async fn test_normaliser_passes_markup_through_delta() {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut textual_block_by_index = std::collections::HashMap::new();
    let mut normaliser = crate::api::stream::StreamTextNormaliser::new();
    let markup = "function=read_file>\nparameter=path>\nsrc/main.rs".to_string();

    forward_conversation_update(
        ConversationStreamUpdate::Delta(markup.clone()),
        &mut textual_block_by_index,
        &mut normaliser,
        &tx,
    );

    match rx.try_recv() {
        Ok(UiUpdate::StreamDelta(text)) => assert_eq!(text, markup),
        Ok(_) => panic!("expected StreamDelta with verbatim markup"),
        Err(_) => panic!("expected StreamDelta with verbatim markup"),
    }
}
