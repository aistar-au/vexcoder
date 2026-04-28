use super::*;
use crate::api::client::MockStreamProducer;
use crate::api::mock_client::MockApiClient;
use crate::runtime::backend::SignalStream;

#[derive(Clone)]
struct OverflowThenSuccessProducer {
    calls: Arc<std::sync::Mutex<usize>>,
    success: MockApiClient,
}

impl MockStreamProducer for OverflowThenSuccessProducer {
    fn create_mock_stream(&self, messages: &[ApiMessage]) -> anyhow::Result<SignalStream> {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        if *calls == 1 {
            return Err(anyhow::anyhow!(
                "request exceeds the available context size (8192 tokens)"
            ));
        }
        self.success.create_mock_stream(messages)
    }
}

#[derive(Clone)]
struct ToolCallThenOverflowUntilMessageCountProducer {
    calls: Arc<std::sync::Mutex<usize>>,
    max_messages: usize,
    initial: MockApiClient,
    success: MockApiClient,
}

impl MockStreamProducer for ToolCallThenOverflowUntilMessageCountProducer {
    fn create_mock_stream(&self, messages: &[ApiMessage]) -> anyhow::Result<SignalStream> {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        if *calls == 1 {
            return self.initial.create_mock_stream(messages);
        }
        if messages.len() > self.max_messages {
            return Err(anyhow::anyhow!(
                "request exceeds the available context size (8192 tokens)"
            ));
        }
        self.success.create_mock_stream(messages)
    }
}

#[derive(Clone)]
struct MultiRoundOverflowProducer {
    calls: Arc<std::sync::Mutex<usize>>,
}

impl MockStreamProducer for MultiRoundOverflowProducer {
    fn create_mock_stream(&self, messages: &[ApiMessage]) -> anyhow::Result<SignalStream> {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        match *calls {
            1 => tagged_text_tool_call_stream(messages, "file1.txt"),
            2 => overflow_error(),
            3 => tagged_text_tool_call_stream(messages, "file2.txt"),
            4 => overflow_error(),
            5 => tagged_text_tool_call_stream(messages, "file3.txt"),
            6 => overflow_error(),
            7 => tagged_text_tool_call_stream(messages, "file4.txt"),
            8 => overflow_error(),
            9 => text_response_stream(messages, "done"),
            _ => unreachable!("unexpected create_stream call count"),
        }
    }
}

fn tagged_text_tool_call_stream(messages: &[ApiMessage], path: &str) -> anyhow::Result<SignalStream> {
    MockApiClient::new(vec![vec![format!(
        r#"data: {{"choices":[{{"delta":{{"content":"<function=read_file>\n<parameter=path>{path}</parameter>\n</function>"}},"finish_reason":"stop"}}]}}"#
    )]])
    .create_mock_stream(messages)
}

fn text_response_stream(messages: &[ApiMessage], text: &str) -> anyhow::Result<SignalStream> {
    MockApiClient::new(vec![vec![format!(
        r#"data: {{"choices":[{{"delta":{{"content":"{text}"}},"finish_reason":"stop"}}]}}"#
    )]])
    .create_mock_stream(messages)
}

fn overflow_error() -> anyhow::Result<SignalStream> {
    Err(anyhow::anyhow!(
        "request exceeds the available context size (8192 tokens)"
    ))
}

#[test]
fn tool_result_signal_uses_recorded_start_time() {
    let mut manager = ConversationManager::new_mock(
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
            vec![],
        ))),
        HashMap::new(),
    );
    let started_at = chrono::DateTime::parse_from_rfc3339("2026-04-16T00:00:00.000Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    manager.record_tool_call_started_at("toolu_01", started_at);
    let signal = manager.tool_result_signal(
        "toolu_01",
        Some("read_file".to_string()),
        "ok".to_string(),
        false,
        started_at + chrono::Duration::milliseconds(250),
    );
    match signal {
        crate::runtime::json_handoff::RuntimeSignal::ToolCallCompleted {
            started_at,
            duration_ms,
            ..
        } => {
            assert_eq!(started_at.as_deref(), Some("2026-04-16T00:00:00.000Z"));
            assert_eq!(duration_ms, Some(250));
        }
        other => panic!("expected ToolCallCompleted, got {other:?}"),
    }
}

#[test]
fn format_tool_result_tracks_read_diff_and_repeat_cycle() {
    let mut manager = ConversationManager::new_mock(
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
            vec![],
        ))),
        HashMap::new(),
    );
    let input = serde_json::json!({ "path": "cal.rs" });
    let first = manager.format_tool_result_for_history(
        "read_file",
        &input,
        &Ok("line1\nline2".to_string()),
    );
    assert!(first.contains("Content for model context:") && first.contains("line1"));

    let second = manager.format_tool_result_for_history(
        "read_file",
        &input,
        &Ok("line1\nline2".to_string()),
    );
    assert!(second.contains("No changes since last read"));

    let third = manager.format_tool_result_for_history(
        "read_file",
        &input,
        &Ok("line1\nchanged".to_string()),
    );
    assert!(third.contains("content changed") && third.contains("line1\nchanged"));
}

#[test]
fn prune_message_history_reanchors_to_user_message() {
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
            vec![],
        ))),
        executor,
    );
    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u0".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a0".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a1".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u1".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a2".to_string()),
            cache_hint: None,
        },
    ];
    manager.prune_message_history(3);
    assert_eq!(manager.api_messages.len(), 2);
    assert_eq!(manager.api_messages[0].role, "user");
    assert_eq!(manager.api_messages[1].role, "assistant");
}

#[test]
fn compact_for_context_overflow_keeps_recent_messages_and_is_noop_when_small() {
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
            vec![],
        ))),
        executor,
    );
    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("old-u0".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("old-a0".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("old-u1".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("old-a1".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("recent-u2".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("recent-a2".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("current-u3".to_string()),
            cache_hint: None,
        },
    ];
    manager.compact_for_context_overflow();
    assert_eq!(manager.api_messages[0].role, "user");
    assert!(format!("{:?}", manager.api_messages.last().unwrap().content).contains("current-u3"));

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u0".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a0".to_string()),
            cache_hint: None,
        },
    ];
    let before = manager.api_messages.len();
    manager.compact_for_context_overflow();
    assert_eq!(manager.api_messages.len(), before);
}

#[test]
fn text_protocol_tool_results_count_as_tool_result_messages() {
    let message = ApiMessage {
        role: "user".to_string(),
        content: Content::Text(
            "tool_result read_file:\nRead foo.rs: 10 chars, 2 lines.".to_string(),
        ),
        cache_hint: None,
    };

    assert!(message_contains_tool_result(&message));
}

#[test]
fn history_pruning_and_overflow_compaction_fallback_to_recent_tool_window() {
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
            vec![],
        ))),
        executor,
    );
    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("original prompt".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("<function=read_file>".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("tool_result read_file:\nchunk one".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("<function=read_file>".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("tool_result read_file:\nchunk two".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("<function=read_file>".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("tool_result read_file:\nchunk three".to_string()),
            cache_hint: None,
        },
    ];

    manager.prune_message_history(4);
    assert_eq!(manager.api_messages.len(), 4);
    assert_eq!(manager.api_messages[0].role, "assistant");

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("original prompt".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("<function=read_file>".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("tool_result read_file:\nchunk one".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("<function=read_file>".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("tool_result read_file:\nchunk two".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("<function=read_file>".to_string()),
            cache_hint: None,
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("tool_result read_file:\nchunk three".to_string()),
            cache_hint: None,
        },
    ];

    manager.compact_for_context_overflow();
    assert_eq!(manager.api_messages.len(), 4);
    assert_eq!(manager.api_messages[0].role, "assistant");
}

#[tokio::test]
async fn context_overflow_retries_even_after_proactive_compaction() -> Result<()> {
    let calls = Arc::new(std::sync::Mutex::new(0));
    let success = MockApiClient::new(vec![vec![
        r#"data: {"choices":[{"delta":{"content":"done"},"finish_reason":"stop"}]}"#.to_string(),
    ]]);
    let client = ApiClient::new_mock(Arc::new(OverflowThenSuccessProducer {
        calls: Arc::clone(&calls),
        success,
    }));
    let mut manager = ConversationManager::new_mock(client, HashMap::new()).with_compaction_config(
        crate::config::CompactionConfig {
            enabled: true,
            threshold_percent: 1,
            keep_recent_turns: 1,
            summary_max_tokens: 32,
        },
    );
    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("old user context".repeat(200)),
            cache_hint: None,
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("old assistant context".repeat(200)),
            cache_hint: None,
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("recent user context".repeat(200)),
            cache_hint: None,
        },
    ];

    let result = manager.send_message("say done".to_string(), None).await?;

    assert_eq!(result, "done");
    assert_eq!(*calls.lock().unwrap(), 2, "must retry once after overflow");
    Ok(())
}

#[tokio::test]
async fn context_overflow_retries_with_smaller_keep_window_when_needed() -> Result<()> {
    let calls = Arc::new(std::sync::Mutex::new(0));
    let initial = MockApiClient::new(vec![vec![
        r#"data: {"choices":[{"delta":{"content":"<function=read_file>\n<parameter=path>file.txt</parameter>\n</function>"},"finish_reason":"stop"}]}"#.to_string(),
    ]]);
    let success = MockApiClient::new(vec![vec![
        r#"data: {"choices":[{"delta":{"content":"done"},"finish_reason":"stop"}]}"#.to_string(),
    ]]);
    let client = ApiClient::new_mock(Arc::new(ToolCallThenOverflowUntilMessageCountProducer {
        calls: Arc::clone(&calls),
        max_messages: 2,
        initial,
        success,
    }));
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file.txt".to_string(), "hello".to_string());
    let mut manager = ConversationManager::new_mock(client, mock_tool_responses);

    let result = manager
        .send_message("What is in file.txt?".to_string(), None)
        .await?;

    assert_eq!(result, "done");
    assert_eq!(
        *calls.lock().unwrap(),
        4,
        "must retry again with a smaller keep window after the first overflow compaction"
    );
    assert!(manager.api_messages.len() <= 3);
    Ok(())
}

#[tokio::test]
async fn context_overflow_retries_reset_after_successful_tool_rounds() -> Result<()> {
    let calls = Arc::new(std::sync::Mutex::new(0));
    let client = ApiClient::new_mock(Arc::new(MultiRoundOverflowProducer {
        calls: Arc::clone(&calls),
    }));
    let mut mock_tool_responses = HashMap::new();
    mock_tool_responses.insert("file1.txt".to_string(), "one".to_string());
    mock_tool_responses.insert("file2.txt".to_string(), "two".to_string());
    mock_tool_responses.insert("file3.txt".to_string(), "three".to_string());
    mock_tool_responses.insert("file4.txt".to_string(), "four".to_string());
    let mut manager = ConversationManager::new_mock(client, mock_tool_responses);

    let result = manager
        .send_message("What is in file1.txt?".to_string(), None)
        .await?;

    assert_eq!(result, "done");
    assert_eq!(
        *calls.lock().unwrap(),
        9,
        "each successful tool round must restore overflow retry budget for the next round"
    );
    Ok(())
}

#[test]
fn clear_messages_resets_cached_conversation_state() {
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
        vec![],
    )));
    let mut manager = ConversationManager::new_mock(client, HashMap::new());
    manager.push_user_message("hello".to_string(), None);
    manager.ensure_task_doc();
    manager.begin_turn_doc("hello".to_string(), PulseToolPolicy::Default);
    manager.clear_messages();
    assert!(manager.messages_for_api().is_empty());
    assert!(manager.task_doc.is_none());
}

#[tokio::test]
async fn send_message_applies_shared_prefix_cache_hint_to_user_turn() {
    let shared_prefix_context =
        "## Shared context prefix\n### File rollups\n- src/lib.rs\n```text\npub fn run() {}\n```\n";
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
        vec![],
    )));
    client.set_supplementary_system_prompt(Some("Respond as a focused Rust reviewer.".to_string()));
    let expected_hint = client
        .shared_prefix_fingerprint(shared_prefix_context)
        .expect("shared prefix fingerprint");
    let mut manager = ConversationManager::new_mock(client, HashMap::new());

    let response = manager
        .send_message_with_policy(
            "what git tools are available?".to_string(),
            None,
            PulseToolPolicy::Default,
            Some(shared_prefix_context.to_string()),
        )
        .await
        .expect("send_message_with_policy");

    assert!(response.contains("git_status"));
    assert_eq!(manager.api_messages[0].role, "user");
    assert_eq!(
        manager.api_messages[0].cache_hint.as_deref(),
        Some(expected_hint.as_str())
    );
}

#[test]
fn truncate_for_history_preserves_head_and_suffix_context() {
    let long = "head-aaaa-bbbb-cccc-dddd-eeee-ffff-gggg-suffix";
    let shortened = truncate_for_history(long, 40);
    assert!(
        shortened.contains("chars omitted")
            && shortened.contains("head")
            && shortened.contains("suffix")
    );

    let short = "abcdefghij";
    assert_eq!(truncate_for_history(short, 40), short);
}
