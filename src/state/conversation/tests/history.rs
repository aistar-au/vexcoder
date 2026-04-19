use super::*;

#[test]
fn test_tool_result_event_uses_recorded_start_time_for_streaming_calls() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let started_at = chrono::DateTime::parse_from_rfc3339("2026-04-16T00:00:00.000Z")
        .expect("fixed timestamp")
        .with_timezone(&chrono::Utc);
    manager.record_tool_call_started_at("toolu_timing_01", started_at);

    let event = manager.tool_result_event(
        "toolu_timing_01",
        Some("read_file".to_string()),
        "ok".to_string(),
        false,
        started_at + chrono::Duration::milliseconds(250),
    );

    match event {
        crate::runtime::json_handoff::RuntimeEvent::ToolCallCompleted {
            started_at,
            completed_at,
            duration_ms,
            ..
        } => {
            assert_eq!(started_at.as_deref(), Some("2026-04-16T00:00:00.000Z"));
            assert_eq!(completed_at, "2026-04-16T00:00:00.250Z");
            assert_eq!(duration_ms, Some(250));
        }
        other => panic!("expected tool_call_completed event, got {other:?}"),
    }

    assert!(!manager.tool_call_started_at.contains_key("toolu_timing_01"));
}
#[test]
fn test_truncate_for_history() {
    let text = "abcdefghij";
    let shortened = truncate_for_history(text, 40);
    assert_eq!(shortened, text);

    let shortened = truncate_for_history(text, 5);
    assert!(shortened.len() <= 5);

    let long_text = "abcdefghijklmnopqrstuvwxyz0123456789";
    let shortened_with_marker = truncate_for_history(long_text, 30);
    assert!(shortened_with_marker.contains("chars omitted"));
}
#[test]
fn test_truncate_for_history_preserves_suffix_context() {
    let text = "head-aaaa-bbbb-cccc-dddd-eeee-ffff-gggg-suffix";
    let shortened = truncate_for_history(text, 40);
    assert!(shortened.contains("chars omitted"));
    assert!(shortened.contains("head"));
    assert!(shortened.contains("suffix"));
}
#[test]
fn test_required_tool_string_validation() {
    let input = json!({ "path": " cal.rs " });
    assert_eq!(
        required_tool_string(&input, "read_file", "path").unwrap(),
        "cal.rs"
    );

    let missing = json!({});
    assert!(required_tool_string(&missing, "read_file", "path").is_err());
}
#[test]
fn test_format_tool_result_for_history_read_file_diff_and_repeat() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());
    let input = serde_json::json!({ "path": "cal.rs" });

    let first = manager.format_tool_result_for_history(
        "read_file",
        &input,
        &Ok("line1\nline2".to_string()),
    );
    assert!(first.contains("Read cal.rs:"));
    assert!(first.contains("Content for model context:"));
    assert!(first.contains("line1\nline2"));

    let second = manager.format_tool_result_for_history(
        "read_file",
        &input,
        &Ok("line1\nline2".to_string()),
    );
    assert!(second.contains("No changes since last read"));

    let third = manager.format_tool_result_for_history(
        "read_file",
        &input,
        &Ok("line1\nline2 changed".to_string()),
    );
    assert!(third.contains("content changed"));
    assert!(third.contains("Content for model context:"));
    assert!(third.contains("line1\nline2 changed"));

    // After a change the cache must update, so the same content read again
    // must be classified as Unchanged — not another Changed.
    let fourth = manager.format_tool_result_for_history(
        "read_file",
        &input,
        &Ok("line1\nline2 changed".to_string()),
    );
    assert!(
        fourth.contains("No changes since last read"),
        "expected Unchanged after re-reading the post-change content, got: {fourth}"
    );
}

#[test]
fn test_format_tool_result_for_history_enriches_listing_output() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());
    let input = serde_json::json!({ "path": "src", "max_entries": 100 });

    let rendered = manager.format_tool_result_for_history(
        "list_files",
        &input,
        &Ok("src/app.rs\nsrc/lib.rs\nsrc/main.rs".to_string()),
    );

    assert!(rendered.contains("Tool list_files completed."));
    assert!(rendered.contains("path: src"));
    assert!(rendered.contains("src/app.rs"));
    assert!(rendered.contains("Result:"));
}

#[test]
fn test_format_tool_result_for_history_enriches_errors_with_request_context() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());
    let input = serde_json::json!({ "path": "src/missing.rs" });

    let rendered = manager.format_tool_result_for_history(
        "read_file",
        &input,
        &Err(anyhow::anyhow!("path not found")),
    );

    assert!(rendered.contains("Tool read_file failed."));
    assert!(rendered.contains("path: src/missing.rs"));
    assert!(rendered.contains("path not found"));
    assert!(rendered.contains("Suggested next step"));
}
#[test]
fn test_append_incremental_suffix_rollup_streaming() {
    let mut content = String::new();
    let a = append_incremental_suffix(&mut content, "Hello");
    let b = append_incremental_suffix(&mut content, "Hello world");
    let c = append_incremental_suffix(&mut content, "Hello world");
    let d = append_incremental_suffix(&mut content, "Hello world!");

    assert_eq!(a, "Hello");
    assert_eq!(b, " world");
    assert_eq!(c, "");
    assert_eq!(d, "!");
    assert_eq!(content, "Hello world!");
}
#[test]
fn test_append_incremental_suffix_handles_unicode_char_boundaries() {
    let mut content = String::new();
    let first = append_incremental_suffix(&mut content, "•");
    let second = append_incremental_suffix(&mut content, "• item");

    assert_eq!(first, "•");
    assert_eq!(second, " item");
    assert_eq!(content, "• item");
}
#[test]
fn test_append_incremental_suffix_keeps_short_suffix_repeat_as_new_delta() {
    let mut content = "The contents of the file are:\n".to_string();
    let appended = append_incremental_suffix(&mut content, "are:\n");
    assert_eq!(appended, "are:\n");
    assert_eq!(content, "The contents of the file are:\nare:\n");
}
#[test]
fn test_prune_message_history_reanchors_to_user() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(mock_api_client, executor);

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u0".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a0".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a1".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u1".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a2".to_string()),
        },
    ];

    manager.prune_message_history(3);

    assert_eq!(manager.api_messages.len(), 2);
    assert_eq!(manager.api_messages[0].role, "user");
    assert_eq!(manager.api_messages[1].role, "assistant");
}
#[test]
fn test_prune_message_history_preserving_keeps_turn_user_anchor() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("anchor user prompt".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("round 1".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("tool_result read_file:\nA".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("round 2".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("tool_result read_file:\nB".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("round 3".to_string()),
        },
    ];

    let anchor_index = 0usize;
    let new_anchor = manager.prune_message_history_preserving(4, anchor_index);
    assert_eq!(
        new_anchor, 0,
        "anchor should be retained at index 0 after pruning"
    );
    assert!(
        matches!(
            &manager.api_messages.first().map(|m| (&m.role, &m.content)),
            Some((role, Content::Text(text))) if role.as_str() == "user" && text == "anchor user prompt"
        ),
        "turn anchor user prompt must be preserved during loop pruning"
    );
}
#[test]
fn test_prune_message_history_preserving_allows_prune_when_anchor_is_far_behind() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("anchor user prompt".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a0".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u1".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a1".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u2".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a2".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u3".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a3".to_string()),
        },
    ];

    let new_anchor = manager.prune_message_history_preserving(4, 0);
    assert_eq!(
        manager.api_messages.len(),
        4,
        "pruning should proceed when anchor is far behind target window"
    );
    assert_eq!(new_anchor, 0);
    assert_eq!(manager.api_messages[0].role, "user");
    match &manager.api_messages[0].content {
        Content::Text(text) => assert_eq!(text, "u2"),
        _ => panic!("expected user text content"),
    }
}
#[test]
fn test_upsert_turn_block_emits_padding_block_starts() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());
    let (tx, mut rx) = mpsc::unbounded_channel();

    manager.upsert_turn_block(
        2,
        StreamBlock::ToolCall {
            id: "toolu_pad_01".to_string(),
            name: "read_file".to_string(),
            input: json!({"path": "file.txt"}),
            status: ToolStatus::Pending,
        },
        Some(&tx),
    );
    drop(tx);

    let mut started_indices = Vec::new();
    while let Ok(update) = rx.try_recv() {
        if let ConversationStreamUpdate::BlockStart { index, block } = update {
            started_indices.push(index);
            if index <= 1 {
                assert!(matches!(
                    block,
                    StreamBlock::Thinking {
                        content,
                        collapsed: true
                    } if content.is_empty()
                ));
            }
        }
    }

    assert_eq!(
        started_indices,
        vec![0, 1, 2],
        "padding blocks and target block must each emit BlockStart"
    );
}
#[test]
fn test_prune_message_history_clears_if_no_user_remains() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(mock_api_client, executor);

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u0".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a0".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a1".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a2".to_string()),
        },
    ];

    manager.prune_message_history(2);
    assert!(manager.api_messages.is_empty());
}
#[test]
fn test_prune_message_history_reanchors_even_if_it_reduces_below_limit() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(mock_api_client, executor);

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u0".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a0".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u1".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a1".to_string()),
        },
    ];

    manager.prune_message_history(3);

    assert_eq!(manager.api_messages.len(), 2);
    assert_eq!(manager.api_messages[0].role, "user");
    if let Content::Text(text) = &manager.api_messages[0].content {
        assert_eq!(text, "u1");
    } else {
        panic!("expected user text content");
    }
}
#[test]
fn test_prune_message_history_skips_leading_tool_result_user_message() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(mock_api_client, executor);

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u0".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Blocks(vec![ContentBlock::ToolUse {
                id: "tool_1".to_string(),
                name: "read_file".to_string(),
                input: json!({"path":"src/lib.rs"}),
                metadata: None,
            }]),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "tool_1".to_string(),
                content: "ok".to_string(),
                is_error: false,
            }]),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a1".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u1".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a2".to_string()),
        },
    ];

    manager.prune_message_history(4);

    assert_eq!(manager.api_messages.len(), 2);
    assert_eq!(manager.api_messages[0].role, "user");
    match &manager.api_messages[0].content {
        Content::Text(text) => assert_eq!(text, "u1"),
        _ => panic!("expected first retained message to be user text, not tool_result"),
    }
}
#[test]
fn test_prune_message_history_clears_if_only_tool_result_user_messages_remain() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(mock_api_client, executor);

    manager.api_messages = vec![
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a0".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "tool_1".to_string(),
                content: "ok".to_string(),
                is_error: false,
            }]),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a1".to_string()),
        },
    ];

    manager.prune_message_history(2);

    assert!(manager.api_messages.is_empty());
}
#[test]
fn test_clear_messages_resets_cached_conversation_state() {
    use crate::runtime::json_handoff::RuntimeEvent;

    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let mut manager = ConversationManager::new_mock(client, HashMap::new());

    manager.push_user_message("hello".to_string());
    manager.ensure_task_doc();
    manager.begin_turn_doc("hello".to_string(), TurnToolPolicy::Default);
    manager.apply_doc_event(RuntimeEvent::TranscriptBlockStart {
        index: 0,
        block: crate::state::stream_block::StreamBlock::FinalText {
            content: "assistant".to_string(),
        },
    });
    assert!(matches!(
        manager
            .read_file_history_cache
            .summarize("src/app.rs", "v1"),
        crate::tool_preview::ReadFileRollupSummary::FirstRead { .. }
    ));
    assert!(matches!(
        manager
            .read_file_history_cache
            .summarize("src/app.rs", "v2"),
        crate::tool_preview::ReadFileRollupSummary::Changed { .. }
    ));

    manager.clear_messages();

    assert!(manager.messages_for_api().is_empty());
    assert!(manager.task_doc.is_none());
    assert!(matches!(
        manager
            .read_file_history_cache
            .summarize("src/app.rs", "v2"),
        crate::tool_preview::ReadFileRollupSummary::FirstRead { .. }
    ));
}
// ---------------------------------------------------------------------------
// Phase 4 — context condensing
// ---------------------------------------------------------------------------

#[test]
fn test_condense_old_tool_results_clips_blocks() {
    let client = Arc::new(MockApiClient::new(vec![]));
    let dir = tempfile::tempdir().unwrap();
    let operator = ToolOperator::new(dir.path().to_path_buf());
    let mut manager = ConversationManager::new(ApiClient::new_mock(client), operator);

    // Build a long tool result.
    let long_result: String = (0..20)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    // Simulate 3 turns: 3 user messages (with tool results), 3 assistant messages.
    for i in 0..3 {
        manager.api_messages.push(ApiMessage {
            role: "assistant".to_string(),
            content: Content::Blocks(vec![ContentBlock::ToolUse {
                id: format!("tool_{i}"),
                name: "read_file".to_string(),
                input: json!({}),
                metadata: None,
            }]),
        });
        manager.api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: Content::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: format!("tool_{i}"),
                content: long_result.clone(),
                is_error: false,
            }]),
        });
    }

    // keep_turns = 1 means only the last turn is preserved at full fidelity.
    manager.condense_old_tool_results(1);

    // First two user messages should be condensed; last should be untouched.
    for (idx, msg) in manager.api_messages.iter().enumerate() {
        if msg.role != "user" {
            continue;
        }
        if let Content::Blocks(blocks) = &msg.content {
            for block in blocks {
                if let ContentBlock::ToolResult { content, .. } = block {
                    if idx < 4 {
                        // Old turns — condensed.
                        assert!(
                            content.contains("more lines"),
                            "expected condensed result at index {idx}, got: {content}"
                        );
                    } else {
                        // Last turn — untouched.
                        assert!(
                            !content.contains("more lines"),
                            "last turn should not be condensed, got: {content}"
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn test_truncate_to_lines_short_input_unchanged() {
    let input = "line 1\nline 2\nline 3";
    let result = super::history::truncate_to_lines(input, 5);
    assert_eq!(result, input);
}
#[test]
fn test_truncate_to_lines_long_input_condensed() {
    let lines: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
    let input = lines.join("\n");
    let result = super::history::truncate_to_lines(&input, 5);
    assert!(result.contains("line 0"));
    assert!(result.contains("line 4"));
    assert!(result.contains("(15 more lines)"));
    assert!(!result.contains("line 5"));
}
#[test]
fn test_truncate_to_lines_is_idempotent() {
    let lines: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
    let input = lines.join("\n");
    let first = super::history::truncate_to_lines(&input, 5);
    let second = super::history::truncate_to_lines(&first, 5);
    assert_eq!(first, second, "truncate_to_lines must be idempotent");
}
#[test]
fn test_compact_for_context_overflow_keeps_recent_messages() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(mock_api_client, executor);

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("old-u0".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("old-a0".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("old-u1".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("old-a1".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("recent-u2".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("recent-a2".to_string()),
        },
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("current-u3".to_string()),
        },
    ];

    manager.compact_for_context_overflow();

    assert_eq!(
        manager.api_messages.len(),
        3,
        "compact should keep last 4 but re-anchor to user: {:?}",
        manager
            .api_messages
            .iter()
            .map(|m| format!("{}:{:?}", m.role, m.content))
            .collect::<Vec<_>>()
    );
    assert_eq!(manager.api_messages[0].role, "user");
    assert!(
        format!("{:?}", manager.api_messages.last().unwrap().content).contains("current-u3"),
        "current user message must be preserved"
    );
}
#[test]
fn test_compact_for_context_overflow_noop_when_small() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(std::path::PathBuf::from("."));
    let mut manager = ConversationManager::new(mock_api_client, executor);

    manager.api_messages = vec![
        ApiMessage {
            role: "user".to_string(),
            content: Content::Text("u0".to_string()),
        },
        ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text("a0".to_string()),
        },
    ];

    manager.compact_for_context_overflow();

    assert_eq!(
        manager.api_messages.len(),
        2,
        "small conversation must not be compacted"
    );
}

#[test]
fn test_compaction_disabled_by_default() {
    let config = crate::config::CompactionConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.threshold_percent, 80);
    assert_eq!(config.keep_recent_turns, 4);
    assert_eq!(config.summary_max_tokens, 1024);
}

#[test]
fn test_compaction_triggers_at_threshold() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    // Each message ~100 chars -> ~25 tokens. 10 messages -> ~250 tokens.
    for i in 0..10 {
        manager.api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: Content::Text(format!("user message number {} with padding text to reach a decent length for token estimation", i)),
        });
        manager.api_messages.push(ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text(format!("assistant response {} with some additional text for token counting purposes here we go", i)),
        });
    }

    let config = crate::config::CompactionConfig {
        enabled: true,
        threshold_percent: 50,
        keep_recent_turns: 4,
        summary_max_tokens: 1024,
    };

    // With context window of 100 tokens and 50% threshold, ~250 tokens should exceed.
    assert!(manager.should_compact_proactively(&config, 100));
    // With a large context window, it should not trigger.
    assert!(!manager.should_compact_proactively(&config, 100_000));
}

#[test]
fn test_compaction_preserves_recent_turns() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    for i in 0..6 {
        manager.api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: Content::Text(format!("user-{i}")),
        });
        manager.api_messages.push(ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text(format!("assistant-{i}")),
        });
    }

    let removed = manager.compact_with_summary(2, "Earlier we discussed topics 0-3.");
    assert!(removed > 0);

    // The summary message + recent 2 turns (4 messages) should be present.
    assert_eq!(manager.api_messages[0].role, "user");
    match &manager.api_messages[0].content {
        Content::Text(t) => assert!(t.contains("[conversation summary]")),
        _ => panic!("expected text content"),
    }

    // Last user message should be one of the recent ones.
    let last_user = manager
        .api_messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .unwrap();
    match &last_user.content {
        Content::Text(t) => assert!(t.contains("user-") || t.contains("[conversation summary]")),
        _ => panic!("expected text content"),
    }
}

#[test]
fn test_compaction_summary_replaces_prefix() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    for i in 0..8 {
        manager.api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: Content::Text(format!("user-{i}")),
        });
        manager.api_messages.push(ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text(format!("assistant-{i}")),
        });
    }

    let before_len = manager.api_messages.len();
    let removed = manager.compact_with_summary(4, "Summary of earlier messages.");
    assert!(removed > 0);
    // Should have summary message (1) + recent 4 turns (8 messages) = 9 total, or close.
    assert!(manager.api_messages.len() < before_len);
    // First message should be the summary.
    match &manager.api_messages[0].content {
        Content::Text(t) => assert!(t.contains("Summary of earlier messages.")),
        _ => panic!("expected text in first message"),
    }
}

#[test]
fn test_compaction_summary_preserves_user_assistant_role_order() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    for i in 0..6 {
        manager.api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: Content::Text(format!("user-{i}")),
        });
        manager.api_messages.push(ApiMessage {
            role: "assistant".to_string(),
            content: Content::Text(format!("assistant-{i}")),
        });
    }

    let removed = manager.compact_with_summary(2, "Earlier discussion summary.");
    assert!(removed > 0);
    assert_eq!(manager.api_messages[0].role, "user");
    assert!(
        manager
            .api_messages
            .windows(2)
            .all(|pair| pair[0].role != pair[1].role)
    );
}

#[test]
fn test_compaction_failure_falls_back_to_full_history() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    // Only 2 user messages - too few to compact.
    manager.api_messages.push(ApiMessage {
        role: "user".to_string(),
        content: Content::Text("user-0".to_string()),
    });
    manager.api_messages.push(ApiMessage {
        role: "assistant".to_string(),
        content: Content::Text("assistant-0".to_string()),
    });
    manager.api_messages.push(ApiMessage {
        role: "user".to_string(),
        content: Content::Text("user-1".to_string()),
    });
    manager.api_messages.push(ApiMessage {
        role: "assistant".to_string(),
        content: Content::Text("assistant-1".to_string()),
    });

    // With keep_recent_turns=4, nothing should be removed.
    let removed = manager.compact_with_summary(4, "summary");
    assert_eq!(removed, 0);
    assert_eq!(manager.api_messages.len(), 4);
}

#[test]
fn test_compaction_config_loads_from_user_layer() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.toml");
    std::fs::write(
        &config_path,
        r#"
model_url = "http://localhost:8080/v1"
model_name = "test"

[compaction]
enabled = true
threshold_percent = 70
keep_recent_turns = 6
summary_max_tokens = 512
"#,
    )
    .unwrap();

    let config =
        crate::config::Config::load_for_tests(dir.path(), Some(&config_path), None).unwrap();

    assert!(config.compaction.enabled);
    assert_eq!(config.compaction.threshold_percent, 70);
    assert_eq!(config.compaction.keep_recent_turns, 6);
    assert_eq!(config.compaction.summary_max_tokens, 512);
}

#[test]
fn test_estimate_history_tokens() {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    // Empty history should have 0 tokens.
    assert_eq!(manager.estimate_history_tokens(), 0);

    // 400 bytes of text = ~100 tokens (400 / 4).
    manager.api_messages.push(ApiMessage {
        role: "user".to_string(),
        content: Content::Text("a".repeat(400)),
    });
    assert_eq!(manager.estimate_history_tokens(), 100);
}
