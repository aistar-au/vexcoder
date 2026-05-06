use super::*;

#[test]
fn tool_round_classification_helpers() {
    let read = vec![ContentBlock::ToolUse {
        id: "t1".to_string(),
        name: "read_file".to_string(),
        input: json!({"path":"src/app/mod.rs"}),
        metadata: None,
    }];
    let write = vec![ContentBlock::ToolUse {
        id: "t2".to_string(),
        name: "write_file".to_string(),
        input: json!({"path":"a","content":"x"}),
        metadata: None,
    }];
    let parallel = vec![
        ContentBlock::ToolUse {
            id: "t3".to_string(),
            name: "read_file".to_string(),
            input: json!({"path":"a"}),
            metadata: None,
        },
        ContentBlock::ToolUse {
            id: "t4".to_string(),
            name: "codebase_search".to_string(),
            input: json!({"query":"q"}),
            metadata: None,
        },
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
    let manager = ConversationManager::new(
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
            vec![],
        ))),
        ToolOperator::new(dir.path().to_path_buf()),
    );
    let result = manager
        .execute_tool_with_timeout_with_updates(
            "read_file",
            &json!({"path": "/nonexistent/path/missing.rs"}),
            Duration::from_secs(5),
            None,
        )
        .await;
    assert!(result.is_err(), "missing file must produce an error result");
    Ok(())
}

#[tokio::test]
async fn multi_read_only_tool_round_reads_correct_content_per_file() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let file_a = dir.path().join("a.txt");
    let file_b = dir.path().join("b.txt");
    std::fs::write(&file_a, "content-a")?;
    std::fs::write(&file_b, "content-b")?;

    let manager = ConversationManager::new(
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
            vec![],
        ))),
        ToolOperator::new(dir.path().to_path_buf()),
    );
    let result_a = manager
        .execute_tool_with_timeout_with_updates(
            "read_file",
            &json!({"path": "a.txt"}),
            Duration::from_secs(5),
            None,
        )
        .await?;
    let result_b = manager
        .execute_tool_with_timeout_with_updates(
            "read_file",
            &json!({"path": "b.txt"}),
            Duration::from_secs(5),
            None,
        )
        .await?;
    assert!(
        result_a.contains("content-a"),
        "tool must read file a correctly"
    );
    assert!(
        result_b.contains("content-b"),
        "tool must read file b correctly"
    );
    Ok(())
}

// Regression for PR #432: execute_parallel_tool_round captured undo snapshots before the
// validation gate ran. When the gate fired (e.g. mutating tool on a read-only request),
// the tool never executed but the snapshot stayed in the map, so a later /undo would try
// to restore a file that was never written.
#[tokio::test]
async fn parallel_tool_round_drops_undo_snapshot_on_validation_failure() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let file_path = dir.path().join("target.txt");
    std::fs::write(&file_path, b"original")?;

    let mut manager = ConversationManager::new(
        ApiClient::new_mock(Arc::new(MockApiClient::new(vec![]))),
        ToolOperator::new(dir.path().to_path_buf()),
    );
    assert!(manager.is_undo_enabled(), "undo must be enabled by default");

    let blocks = vec![ContentBlock::ToolUse {
        id: "tool-1".to_string(),
        name: "write_file".to_string(),
        input: json!({"path": "target.txt", "content": "should not be written"}),
        metadata: None,
    }];

    let completed = manager
        .execute_parallel_tool_round(
            &blocks,
            "read-only inspect target.txt",
            Duration::from_secs(5),
            true,
            None,
        )
        .await;

    assert_eq!(completed.len(), 1);
    assert!(
        completed[0].result.is_err(),
        "mutating call on a read-only request must be blocked by the validation gate"
    );
    assert_eq!(
        manager.undo_stack_len(),
        0,
        "validation failure must not leak an undo checkpoint onto the stack"
    );
    assert_eq!(
        std::fs::read(&file_path)?,
        b"original",
        "the gated tool must not have written the target file"
    );
    Ok(())
}
