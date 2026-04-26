use super::RuntimeContext;
use crate::api::{ApiClient, mock_client::MockApiClient};
use crate::runtime::UiUpdate;
use crate::state::ConversationManager;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn start_pulse_dispatches_stream_delta_and_completion() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n".to_string(),
        "data: {\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}]}\n\n"
            .to_string(),
    ]])));
    let mut ctx = RuntimeContext::new(
        ConversationManager::new_mock(client, HashMap::new()),
        tx,
        CancellationToken::new(),
    );
    ctx.start_pulse("test input".to_string());
    let mut saw_delta = false;
    let mut saw_complete = false;
    loop {
        match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(UiUpdate::StreamDelta(_))) | Ok(Some(UiUpdate::StreamBlockDelta { .. })) => {
                saw_delta = true
            }
            Ok(Some(UiUpdate::PulseComplete)) => {
                saw_complete = true;
                break;
            }
            Ok(Some(UiUpdate::Error(e))) => panic!("error: {e}"),
            Ok(None) | Err(_) => break,
            _ => {}
        }
    }
    assert!(saw_delta && saw_complete);
}

#[test]
fn start_pulse_without_tokio_runtime_emits_error() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut ctx = RuntimeContext::new(
        ConversationManager::new_mock(
            ApiClient::new_mock(Arc::new(MockApiClient::new(vec![]))),
            HashMap::new(),
        ),
        tx,
        CancellationToken::new(),
    );
    ctx.start_pulse("test".to_string());
    match rx.try_recv().expect("expected error") {
        UiUpdate::Error(msg) => {
            assert!(msg.contains("requires active Tokio runtime"), "got: {msg}")
        }
        _ => panic!("expected UiUpdate::Error"),
    }
}
