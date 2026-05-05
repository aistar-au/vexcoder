use super::*;

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

// Regression: anchor heuristic previously allowed keep_start < target_keep_start,
// leaving len - preserve_index > max_api_messages messages and triggering repeated
// context-overflow retry loops in send_message_with_policy.
#[test]
fn prune_message_history_preserving_anchor_never_exceeds_max() {
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(
        ApiClient::new_mock(Arc::new(MockApiClient::new(vec![]))),
        executor,
    );

    // 20 messages: alternating user / assistant
    for i in 0..10 {
        manager.api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: Content::Text(format!("u{i}")),
            cache_hint: None,
        });
        manager.api_messages.push(ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text(format!("a{i}")),
            cache_hint: None,
        });
    }
    assert_eq!(manager.api_messages.len(), 20);

    // max=14  → target_keep_start = 20 − 14 = 6
    // preserve_index=5, distance=1 ≤ 2 → anchor fires, pre-fix keep_start=5 → 15 msgs
    let _new_index = manager.prune_message_history_preserving(14, 5);

    assert!(
        manager.api_messages.len() <= 14,
        "expected ≤ 14 messages after prune, got {}",
        manager.api_messages.len()
    );
    assert_eq!(
        manager.api_messages[0].role,
        "user",
        "first kept message must be a clean user turn"
    );
}

// Regression: undo snapshots were captured before validation in execute_parallel_tool_round.
// A validation failure (e.g., mutating call on a read-only request) left orphaned snapshots,
// causing /undo to attempt restoring files that were never modified.
#[tokio::test]
async fn parallel_tool_round_drops_snapshot_on_validation_failure() {
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(
        ApiClient::new_mock(Arc::new(MockApiClient::new(vec![]))),
        executor,
    );
    assert!(manager.is_undo_enabled());

    // write_file with a path → capture_undo_snapshot returns Some
    // "show me" → is_read_only_user_request=true → mutating_tool_read_only_conflict fires
    let blocks = vec![ContentBlock::ToolUse {
        id: "toolu_snap_01".to_string(),
        name: "write_file".to_string(),
        input: json!({ "path": "/tmp/vex_snapshot_leak_test.txt", "content": "x" }),
        metadata: None,
    }];

    manager
        .execute_parallel_tool_round(&blocks, "show me the diff", Duration::from_secs(5), false, None)
        .await;

    assert_eq!(
        manager.undo_stack_len(),
        0,
        "undo snapshot must not be pushed when the tool was blocked by validation"
    );
}
