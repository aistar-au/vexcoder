use super::*;

#[test]
fn tool_result_event_uses_recorded_start_time() {
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
    let event = manager.tool_result_event(
        "toolu_01",
        Some("read_file".to_string()),
        "ok".to_string(),
        false,
        started_at + chrono::Duration::milliseconds(250),
    );
    match event {
        crate::runtime::json_handoff::RuntimeEvent::ToolCallCompleted {
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

    // noop when few messages
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
fn clear_messages_resets_cached_conversation_state() {
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
        vec![],
    )));
    let mut manager = ConversationManager::new_mock(client, HashMap::new());
    manager.push_user_message("hello".to_string());
    manager.ensure_task_doc();
    manager.begin_turn_doc("hello".to_string(), PulseToolPolicy::Default);
    manager.clear_messages();
    assert!(manager.messages_for_api().is_empty());
    assert!(manager.task_doc.is_none());
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
