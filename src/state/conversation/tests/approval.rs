use super::*;

#[tokio::test]
async fn test_rename_file_refreshes_codebase_search_index() -> Result<()> {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    let temp = TempDir::new()?;
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir)?;
    std::fs::write(src_dir.join("old_name.rs"), "pub fn renamed_symbol() {}\n")?;

    rebuild_codebase_index_for_tests(temp.path());

    let operator = ToolOperator::new(temp.path().to_path_buf());
    let before = execute_tool_dispatch(
        &operator,
        "codebase_search",
        &json!({
            "query": "renamed_symbol",
            "max_results": 5
        }),
    )?;
    assert!(before.contains("src/old_name.rs:1-1"));

    let rename_result = execute_tool_dispatch(
        &operator,
        "rename_file",
        &json!({
            "old_path": "src/old_name.rs",
            "new_path": "src/new_name.rs"
        }),
    )?;
    assert!(rename_result.contains("Renamed src/old_name.rs -> src/new_name.rs"));

    let after = execute_tool_dispatch(
        &operator,
        "codebase_search",
        &json!({
            "query": "renamed_symbol",
            "max_results": 5
        }),
    )?;
    assert!(after.contains("src/new_name.rs:1-1"));
    assert!(!after.contains("src/old_name.rs:1-1"));

    Ok(())
}
#[tokio::test]
async fn test_execute_tool_codebase_search_works_without_embeddings() -> Result<()> {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    std::env::remove_var("VEX_EMBEDDING_PROVIDER");
    std::env::remove_var("VEX_EMBEDDING_MODEL");
    std::env::remove_var("VEX_EMBEDDING_URL");
    std::env::remove_var("VEX_EMBEDDING_API_KEY");

    let temp = TempDir::new()?;
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir)?;
    std::fs::write(
        src_dir.join("searchable.rs"),
        "pub fn semantic_fallback() {}\n",
    )?;

    rebuild_codebase_index_for_tests(temp.path());

    let mock_api_client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let manager = ConversationManager::new(
        mock_api_client,
        ToolOperator::new(temp.path().to_path_buf()),
    );
    let result = manager
        .execute_tool_with_timeout_with_updates(
            "codebase_search",
            &json!({
                "query": "semantic_fallback",
                "max_results": 5,
            }),
            Duration::from_secs(2),
            None,
        )
        .await?;

    assert!(result.contains("semantic_fallback"));
    assert!(result.contains("src/searchable.rs:1-1"));
    Ok(())
}

#[tokio::test]
async fn test_execute_tool_codebase_search_rejects_disabled_search_config() -> Result<()> {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    let temp = TempDir::new()?;
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir)?;
    std::fs::write(
        src_dir.join("searchable.rs"),
        "pub fn disabled_search() {}\n",
    )?;

    let mock_api_client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let manager = ConversationManager::new(
        mock_api_client,
        ToolOperator::new(temp.path().to_path_buf()),
    )
    .with_search_config(crate::config::SearchConfig {
        enabled: false,
        ..Default::default()
    });

    let error = manager
        .execute_tool_with_timeout_with_updates(
            "codebase_search",
            &json!({
                "query": "disabled_search",
                "max_results": 5,
            }),
            Duration::from_secs(2),
            None,
        )
        .await
        .expect_err("disabled search must reject codebase_search");

    assert!(error
        .to_string()
        .contains("codebase_search is disabled by [search].enabled=false"));
    Ok(())
}

#[tokio::test]
async fn test_incremental_refresh_does_not_depend_on_auto_index() -> Result<()> {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    crate::state::clear_codebase_index_for_tests();

    let temp = TempDir::new()?;
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir)?;
    let file_path = src_dir.join("searchable.rs");
    std::fs::write(&file_path, "pub fn before_auto_index_off() {}\n")?;

    let mock_api_client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let manager = ConversationManager::new(
        mock_api_client,
        ToolOperator::new(temp.path().to_path_buf()),
    )
    .with_search_config(crate::config::SearchConfig {
        auto_index: false,
        ..Default::default()
    });

    let before = manager
        .execute_tool_with_timeout_with_updates(
            "codebase_search",
            &json!({
                "query": "before_auto_index_off",
                "max_results": 5,
            }),
            Duration::from_secs(2),
            None,
        )
        .await?;
    assert!(before.contains("before_auto_index_off"));

    manager
        .execute_tool_with_timeout_with_updates(
            "write_file",
            &json!({
                "path": "src/searchable.rs",
                "content": "pub fn after_auto_index_off() {}\n",
            }),
            Duration::from_secs(2),
            None,
        )
        .await?;

    let after = manager
        .execute_tool_with_timeout_with_updates(
            "codebase_search",
            &json!({
                "query": "after_auto_index_off",
                "max_results": 5,
            }),
            Duration::from_secs(2),
            None,
        )
        .await?;

    assert!(after.contains("after_auto_index_off"));
    assert!(!after.contains("before_auto_index_off"));
    Ok(())
}

#[test]
fn test_default_tool_approval_enabled_prefers_remote_only() {
    assert!(default_tool_approval_enabled(false));
    assert!(!default_tool_approval_enabled(true));
}
#[test]
fn test_env_bool_off_is_false_across_state_paths() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_STREAM_LOCAL_TOOL_EVENTS", "off");
    std::env::set_var("VEX_STREAM_SERVER_EVENTS", "off");
    std::env::set_var("VEX_TOOL_CONFIRM", "off");

    assert!(!stream_local_tool_events_enabled());
    assert!(!stream_server_events_enabled());
    assert!(!tool_approval_enabled(false));

    std::env::remove_var("VEX_STREAM_LOCAL_TOOL_EVENTS");
    std::env::remove_var("VEX_STREAM_SERVER_EVENTS");
    std::env::remove_var("VEX_TOOL_CONFIRM");
}
#[tokio::test]
async fn test_mutating_tool_prompts_approval_when_tool_confirm_env_is_off() -> Result<()> {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    std::env::set_var("VEX_TOOL_CONFIRM", "off");

    let first_response_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mut_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_mut_01","name":"write_file","input":{"path":"calculator.rs","content":"fn main() {}\n"}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":4}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];

    let second_response_sse = plain_text_round("msg_mut_02", "No changes were applied.");
    let mock_api_client =
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
    let mut manager = ConversationManager::new_mock(mock_api_client, HashMap::new());

    let (tx, mut rx) = mpsc::unbounded_channel();
    let approval_task = tokio::spawn(async move {
        let mut saw_approval_request = false;
        while let Some(update) = rx.recv().await {
            if let ConversationStreamUpdate::ToolApprovalRequest(request) = update {
                saw_approval_request = true;
                let _ = request.response_tx.send(false);
            }
        }
        saw_approval_request
    });
    let final_text = manager
        .send_message("create calculator.rs".to_string(), Some(&tx))
        .await?;
    drop(tx);
    let saw_approval_request = approval_task.await?;

    std::env::remove_var("VEX_TOOL_CONFIRM");
    assert!(saw_approval_request);
    assert!(final_text.contains("No changes were applied."));
    let tool_result_message = manager
        .api_messages
        .iter()
        .find(|message| {
            message.role == "user"
                && matches!(message.content, Content::Blocks(_))
                && message_contains_tool_result(message)
        })
        .expect("expected tool_result message in history");
    if let Content::Blocks(blocks) = &tool_result_message.content {
        assert!(blocks.iter().any(|block| matches!(
            block,
            ContentBlock::ToolResult {
                tool_use_id,
                is_error: true,
                ..
            } if tool_use_id == "toolu_mut_01"
        )));
    } else {
        panic!("expected tool_result blocks");
    }
    Ok(())
}
