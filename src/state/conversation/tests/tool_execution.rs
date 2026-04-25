use super::*;

#[test]
fn tool_round_classification_helpers() {
    let read = vec![ContentBlock::ToolUse { id: "t1".to_string(), name: "read_file".to_string(), input: json!({"path":"src/app/mod.rs"}), metadata: None }];
    let write = vec![ContentBlock::ToolUse { id: "t2".to_string(), name: "write_file".to_string(), input: json!({"path":"a","content":"x"}), metadata: None }];
    let parallel = vec![
        ContentBlock::ToolUse { id: "t3".to_string(), name: "read_file".to_string(), input: json!({"path":"a"}), metadata: None },
        ContentBlock::ToolUse { id: "t4".to_string(), name: "codebase_search".to_string(), input: json!({"query":"q"}), metadata: None },
    ];

    assert!(is_read_only_tool_round(&read));
    assert!(!is_read_only_tool_round(&write));
    assert!(is_parallel_safe_tool_round(&parallel));
    assert!(!is_parallel_safe_tool_round(&write));
    assert!(should_parallelize_tool_round(&parallel, false));
    assert!(!should_parallelize_tool_round(&parallel, true));

    let sig_a = tool_round_signature(&read);
    let sig_b = tool_round_signature(&read);
    assert_eq!(sig_a, sig_b);
    assert!(tool_requires_confirmation("write_file"));
    assert!(!tool_requires_confirmation("read_file"));
}

#[tokio::test]
async fn tool_execution_error_sets_error_status() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut manager = ConversationManager::new(
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![]))),
        ToolOperator::new(dir.path().to_path_buf()),
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    manager.execute_tool(
        &ContentBlock::ToolUse { id: "tc1".to_string(), name: "read_file".to_string(), input: json!({"path":"/nonexistent/path/missing.rs"}), metadata: None },
        tx,
        None,
    ).await;
    let mut saw_error = false;
    while let Ok(update) = rx.try_recv() {
        if let ConversationStreamUpdate::ToolResult { status, .. } = update {
            if matches!(status, ToolStatus::Error) { saw_error = true; }
        }
    }
    assert!(saw_error, "missing file must produce a ToolStatus::Error result");
    Ok(())
}

#[tokio::test]
async fn multi_read_only_tool_round_preserves_result_order() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let file_a = dir.path().join("a.txt");
    let file_b = dir.path().join("b.txt");
    std::fs::write(&file_a, "content-a")?;
    std::fs::write(&file_b, "content-b")?;

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut manager = ConversationManager::new(
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![]))),
        ToolOperator::new(dir.path().to_path_buf()),
    );
    let round = vec![
        ContentBlock::ToolUse { id: "ta".to_string(), name: "read_file".to_string(), input: json!({"path": file_a}), metadata: None },
        ContentBlock::ToolUse { id: "tb".to_string(), name: "read_file".to_string(), input: json!({"path": file_b}), metadata: None },
    ];
    manager.execute_tool_round(&round, tx, None).await;
    let mut ids = Vec::new();
    while let Ok(update) = rx.try_recv() {
        if let ConversationStreamUpdate::ToolResult { tool_use_id, .. } = update {
            ids.push(tool_use_id);
        }
    }
    assert_eq!(ids, vec!["ta".to_string(), "tb".to_string()]);
    Ok(())
}
