use super::*;
use crate::runtime::edit_loop::EditLoopOutcome;

#[tokio::test]
async fn batch_mode_exits_zero_on_completion() {
    let result = run_batch_mode("echo hello", 3).await.unwrap();
    assert_eq!(result.status, TaskStatus::Completed);
}

#[test]
fn batch_mode_ignores_non_task_ui_updates() {
    use crate::api::mock_client::MockApiClient;
    use std::collections::HashMap;
    use std::sync::Arc;

    let mut mode = BatchMode::new("task".to_string(), BatchRunOpts::default(), None, None);
    let client =
        crate::api::ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let conversation =
        crate::state::ConversationManager::new_mock(client, HashMap::new());
    let (tx, _rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut ctx =
        RuntimeContext::new(conversation, tx, CancellationToken::new());

    mode.on_model_update(UiUpdate::TranscriptLine("noise".to_string()), &mut ctx);
    mode.on_model_update(
        UiUpdate::EditLoopComplete {
            outcome: EditLoopOutcome::Cancelled,
            last_validation_result: None,
        },
        &mut ctx,
    );

    assert!(mode.current_response.is_empty());
    assert!(mode.output_lines.is_empty());
    assert_eq!(mode.status, TaskStatus::Ready);
}

#[tokio::test]
async fn batch_mode_memory_clear_requires_auto_approve() {
    let result = run_batch_mode_with_opts(
        "/memory clear",
        BatchRunOpts {
            format: OutputFormat::Text,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(result.status, TaskStatus::Failed);
    assert!(
        result
            .output_lines
            .iter()
            .any(|line| line.contains("requires --auto-approve")),
    );
}

#[tokio::test]
async fn batch_mode_jsonl_output_has_required_fields() {
    let output = capture_batch_jsonl("echo hello", 3).await.unwrap();
    let records: Vec<serde_json::Value> =
        output.lines().map(|l| serde_json::from_str(l).unwrap()).collect();

    let pulse = records
        .iter()
        .find(|r| !r["summary"].as_bool().unwrap_or(false))
        .expect("pulse record");
    assert_eq!(pulse["pulse"], 1);
    assert!(pulse["changed_files"].is_array());

    let summary = records
        .iter()
        .find(|r| r["summary"].as_bool() == Some(true))
        .expect("summary record");
    assert_eq!(summary["status"], "Completed");
}

#[tokio::test]
async fn batch_mode_zero_max_turns_produces_max_turns_reached() {
    let result = run_batch_mode_with_opts(
        "task",
        BatchRunOpts {
            max_turns: Some(0),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(result.status, TaskStatus::MaxTurnsReached);
}
