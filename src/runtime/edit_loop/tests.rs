use super::*;
use crate::api::{ApiClient, mock_client::MockApiClient};
use crate::runtime::UiUpdate;
use crate::state::ConversationManager;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

fn make_runtime_context() -> (RuntimeContext, mpsc::UnboundedReceiver<UiUpdate>) {
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    let (tx, rx) = mpsc::unbounded_channel();
    (RuntimeContext::new(conversation, tx, CancellationToken::new()), rx)
}

#[tokio::test]
async fn edit_loop_terminates_at_max_turns() {
    let mut edit_loop = EditLoop::new("task-001".to_string()).with_max_turns(1);
    let (mut ctx, _rx) = make_runtime_context();
    let outcome = edit_loop.run("edit src/runtime/edit_loop.rs".to_string(), &mut ctx, &CancellationToken::new()).await.unwrap();
    assert!(matches!(outcome, EditLoopOutcome::MaxTurnsReached { .. }));
}

#[tokio::test]
async fn edit_loop_returns_cancelled_when_token_is_pre_cancelled() {
    let mut edit_loop = EditLoop::new("task-002".to_string());
    let (mut ctx, _rx) = make_runtime_context();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let outcome = edit_loop.run("edit src/runtime/edit_loop.rs".to_string(), &mut ctx, &cancel).await.unwrap();
    assert!(matches!(outcome, EditLoopOutcome::Cancelled));
}
