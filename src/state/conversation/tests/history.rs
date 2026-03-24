use super::*;

#[test]
fn test_parse_tagged_tool_calls() {
    let text = r#"I can do this.
<function=write_file>
<parameter=path>
cal.rs
</parameter>
<parameter=content>
fn main() {}
</parameter>
</function>"#;

    let calls = parse_tagged_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "write_file");
    assert_eq!(calls[0].input["path"], "cal.rs");
    assert_eq!(calls[0].input["content"], "fn main() {}");
}
#[test]
fn test_parse_tagged_tool_calls_without_parameters() {
    let text = "Checking files.\n<function=list_files></function>";
    let calls = parse_tagged_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "list_files");
    assert_eq!(calls[0].input, json!({}));
}
#[test]
fn test_parse_tagged_tool_calls_with_missing_closing_tags() {
    let text = r#"I'll check it.
<function=read_file>
<parameter=path>
cal.js
"#;
    let calls = parse_tagged_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "read_file");
    assert_eq!(calls[0].input["path"], "cal.js");
}
#[test]
fn test_truncate_for_history() {
    let text = "abcdefghij";
    let truncated = truncate_for_history(text, 40);
    assert_eq!(truncated, text);

    let truncated = truncate_for_history(text, 5);
    assert!(truncated.len() <= 5);

    let long_text = "abcdefghijklmnopqrstuvwxyz0123456789";
    let truncated_with_marker = truncate_for_history(long_text, 30);
    assert!(truncated_with_marker.contains("[truncated"));
}
#[test]
fn test_truncate_for_history_preserves_tail_context() {
    let text = "head-aaaa-bbbb-cccc-dddd-eeee-ffff-gggg-tail";
    let truncated = truncate_for_history(text, 40);
    assert!(truncated.contains("[truncated"));
    assert!(truncated.contains("head"));
    assert!(truncated.contains("tail"));
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
fn test_append_incremental_suffix_snapshot_streaming() {
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
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let mut manager = ConversationManager::new_mock(client, HashMap::new());

    manager.push_user_message("hello".to_string());
    manager.current_turn_blocks.push(StreamBlock::FinalText {
        content: "assistant".to_string(),
    });
    assert!(matches!(
        manager
            .read_file_history_cache
            .summarize("src/app.rs", "v1"),
        crate::tool_preview::ReadFileSnapshotSummary::FirstRead { .. }
    ));
    assert!(matches!(
        manager
            .read_file_history_cache
            .summarize("src/app.rs", "v2"),
        crate::tool_preview::ReadFileSnapshotSummary::Changed { .. }
    ));

    manager.clear_messages();

    assert!(manager.messages_for_api().is_empty());
    assert!(manager.current_turn_blocks.is_empty());
    assert!(matches!(
        manager
            .read_file_history_cache
            .summarize("src/app.rs", "v2"),
        crate::tool_preview::ReadFileSnapshotSummary::FirstRead { .. }
    ));
}
// ---------------------------------------------------------------------------
// Phase 4 — context condensing
// ---------------------------------------------------------------------------

#[test]
fn test_condense_old_tool_results_truncates_blocks() {
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
