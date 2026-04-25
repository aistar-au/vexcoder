use super::*;

#[test]
fn missing_read_only_location_prompt_requires_explicit_paths() {
    let prompt = missing_read_only_location_prompt("read_file", &json!({})).expect("must prompt for missing path");
    assert!(prompt.contains("explicit file path") && prompt.contains("[file: ...]"));
    assert!(missing_read_only_location_prompt("read_file", &json!({"path":"   "})).is_some());
    assert!(missing_read_only_location_prompt("read_file", &json!({"path":"src/calculator.rs"})).is_none());
    assert!(missing_read_only_location_prompt("edit_file", &json!({})).is_none());
}

#[test]
fn read_only_user_request_detection_and_mutating_guard() {
    assert!(is_read_only_user_request("show me calculator.rs"));
    assert!(is_read_only_user_request("what is in src/runtime/loop.rs"));
    assert!(!is_read_only_user_request("add a new function and commit it"));

    let guard = mutating_tool_read_only_conflict_prompt("show the git diff", "write_file");
    assert!(guard.is_some());
    assert!(guard.unwrap().contains("No file changes were made"));

    assert!(mutating_tool_read_only_conflict_prompt("add calculator.rs", "write_file").is_none());
    assert!(mutating_tool_read_only_conflict_prompt("read-only inspect src/app.rs", "edit_file").is_some());
}

#[tokio::test]
async fn read_file_missing_path_returns_clarification_not_loop() -> Result<()> {
    let first_sse = vec![
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_rp_01","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_rp_01","name":"read_file","input":{}}}"#.to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#.to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":3}}"#.to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
    ];
    let second_sse = plain_text_round("msg_rp_02", "Please specify the file path you want to read.");
    let mut manager = ConversationManager::new_mock(
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![first_sse, second_sse]))),
        HashMap::new(),
    );
    let final_text = manager.send_message("read a file".to_string(), None).await?;
    assert!(final_text.contains("file path"), "guard response must ask for path; got: {final_text}");
    Ok(())
}
