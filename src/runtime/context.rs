use crate::runtime::{EditLoop, UiUpdate};
use crate::state::{ConversationManager, ConversationStreamUpdate, StreamBlock, TurnToolPolicy};
use crate::types::{Content, ContentBlock};
use crate::usage::{estimate_tokens, SessionTokens, TurnTokens};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;

pub struct RuntimeContext {
    conversation: Arc<Mutex<ConversationManager>>,
    update_tx: mpsc::UnboundedSender<UiUpdate>,
    cancel: CancellationToken,
    session_tokens: Arc<StdMutex<SessionTokens>>,
}

impl Clone for RuntimeContext {
    fn clone(&self) -> Self {
        Self {
            conversation: Arc::clone(&self.conversation),
            update_tx: self.update_tx.clone(),
            cancel: self.cancel.clone(),
            session_tokens: Arc::clone(&self.session_tokens),
        }
    }
}

impl RuntimeContext {
    pub fn new(
        conversation: ConversationManager,
        update_tx: mpsc::UnboundedSender<UiUpdate>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            conversation: Arc::new(Mutex::new(conversation)),
            update_tx,
            cancel,
            session_tokens: Arc::new(StdMutex::new(SessionTokens::default())),
        }
    }

    pub fn start_turn(&mut self, input: String) {
        self.start_turn_with_system_prompt(input, None);
    }

    pub fn start_turn_with_system_prompt(
        &mut self,
        input: String,
        supplementary_system_prompt: Option<String>,
    ) {
        self.start_turn_with_system_prompt_and_policy(
            input,
            supplementary_system_prompt,
            TurnToolPolicy::Default,
        );
    }

    pub fn start_turn_with_system_prompt_and_policy(
        &mut self,
        input: String,
        supplementary_system_prompt: Option<String>,
        turn_tool_policy: TurnToolPolicy,
    ) {
        if tokio::runtime::Handle::try_current().is_err() {
            let _ = self.update_tx.send(UiUpdate::Error(
                "runtime error: start_turn requires active Tokio runtime".to_string(),
            ));
            return;
        }

        let turn_cancel = self.cancel.child_token();
        let tx = self.update_tx.clone();
        let conversation = Arc::clone(&self.conversation);
        let session_tokens = Arc::clone(&self.session_tokens);
        let input_for_estimate = input.clone();

        tokio::spawn(async move {
            set_runtime_prompt(&conversation, supplementary_system_prompt).await;
            let (delta_tx, mut delta_rx) = mpsc::unbounded_channel::<ConversationStreamUpdate>();
            let conversation_for_send = Arc::clone(&conversation);

            let send_handle = tokio::spawn(async move {
                let mut mgr = conversation_for_send.lock().await;
                let result = mgr
                    .send_message_with_policy(input, Some(&delta_tx), turn_tool_policy)
                    .await;
                let turn_tokens = mgr.take_last_turn_tokens();
                (result, turn_tokens)
            });

            let mut textual_block_by_index = std::collections::HashMap::<usize, bool>::new();
            let mut cancelled = false;

            loop {
                tokio::select! {
                    _ = turn_cancel.cancelled() => {
                        send_handle.abort();
                        cancelled = true;
                        break;
                    }
                    update = delta_rx.recv() => {
                        match update {
                            Some(update) => forward_conversation_update(update, &mut textual_block_by_index, &tx),
                            None => break,
                        }
                    }
                }
            }

            let send_result = if cancelled {
                None
            } else {
                Some(send_handle.await)
            };

            set_runtime_prompt(&conversation, None).await;

            if cancelled {
                let _ = tx.send(UiUpdate::TurnComplete);
                return;
            }

            match send_result.expect("non-cancelled turn must await send_handle") {
                Ok((Ok(response_text), turn_tokens)) => {
                    let recorded =
                        normalize_turn_tokens(&input_for_estimate, &response_text, turn_tokens);
                    if let Ok(mut tokens) = session_tokens.lock() {
                        tokens.record_turn(recorded);
                    }
                    let _ = tx.send(UiUpdate::TurnComplete);
                }
                Ok((Err(e), _)) => {
                    let _ = tx.send(UiUpdate::Error(e.to_string()));
                }
                Err(e) => {
                    if e.is_cancelled() {
                        let _ = tx.send(UiUpdate::TurnComplete);
                    } else {
                        let _ = tx.send(UiUpdate::Error(e.to_string()));
                    }
                }
            }
        });
    }

    pub fn start_edit_loop(&mut self, mut edit_loop: EditLoop, instruction: String) {
        if tokio::runtime::Handle::try_current().is_err() {
            let _ = self.update_tx.send(UiUpdate::Error(
                "runtime error: start_edit_loop requires active Tokio runtime".to_string(),
            ));
            return;
        }

        let loop_cancel = self.cancel.child_token();
        let tx = self.update_tx.clone();
        let mut loop_ctx = self.clone();

        tokio::spawn(async move {
            let system_prompt = edit_loop
                .profile
                .system_prompt_text()
                .expect("edit loop profile system prompt must resolve");
            set_runtime_prompt(&loop_ctx.conversation, Some(system_prompt.to_string())).await;

            let result = edit_loop
                .run(instruction, &mut loop_ctx, &loop_cancel)
                .await;

            set_runtime_prompt(&loop_ctx.conversation, None).await;

            match result {
                Ok(outcome) => {
                    let _ = tx.send(UiUpdate::EditLoopComplete {
                        outcome,
                        last_validation_result: edit_loop.last_validation_result().cloned(),
                    });
                }
                Err(err) => {
                    let _ = tx.send(UiUpdate::Error(err.to_string()));
                }
            }
        });
    }

    pub fn set_model_name(&self, name: String) -> Result<(), &'static str> {
        let conversation = self
            .conversation
            .try_lock()
            .map_err(|_| "model switch unavailable while runtime state is busy")?;
        conversation.set_model_name(name);
        Ok(())
    }

    #[cfg(test)]
    pub fn test_message_count_try_lock(&self) -> Option<usize> {
        self.conversation
            .try_lock()
            .ok()
            .map(|mgr| mgr.messages_for_api().len())
    }

    #[cfg(test)]
    pub async fn test_message_count(&self) -> usize {
        self.conversation.lock().await.messages_for_api().len()
    }

    #[cfg(test)]
    pub fn test_root_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    #[cfg(test)]
    pub async fn test_system_prompt(&self) -> String {
        let manager = self.conversation.lock().await;
        manager.client().test_system_prompt()
    }

    #[cfg(test)]
    pub async fn test_model_name(&self) -> String {
        self.conversation.lock().await.model_name()
    }

    #[cfg(test)]
    pub fn test_record_session_turn(&self, turn: TurnTokens) {
        if let Ok(mut tokens) = self.session_tokens.lock() {
            tokens.record_turn(turn);
        }
    }

    pub fn cancel_turn(&mut self) {
        self.cancel.cancel();
        self.cancel = CancellationToken::new();
    }

    pub fn turn_cancellation_token(&self) -> CancellationToken {
        self.cancel.child_token()
    }

    pub fn emit_turn_complete(&self) {
        let _ = self.update_tx.send(UiUpdate::TurnComplete);
    }

    pub fn clear_conversation(&self) {
        if let Ok(mut conversation) = self.conversation.try_lock() {
            conversation.clear_messages();
            return;
        }

        if tokio::runtime::Handle::try_current().is_ok() {
            let conversation = Arc::clone(&self.conversation);
            tokio::spawn(async move {
                conversation.lock().await.clear_messages();
            });
            return;
        }

        self.conversation.blocking_lock().clear_messages();
    }

    pub fn emit_transcript_line(&self, line: String) {
        let _ = self.update_tx.send(UiUpdate::TranscriptLine(line));
    }

    pub fn session_tokens_snapshot(&self) -> SessionTokens {
        self.session_tokens
            .lock()
            .map(|tokens| *tokens)
            .unwrap_or_default()
    }

    pub fn reset_session_tokens(&self) {
        if let Ok(mut tokens) = self.session_tokens.lock() {
            tokens.reset();
        }
    }

    pub fn estimated_conversation_tokens(&self) -> usize {
        self.conversation
            .try_lock()
            .ok()
            .map(|conversation| estimate_token_count(&conversation.messages_for_api()))
            .unwrap_or(0)
    }
}

async fn set_runtime_prompt(
    conversation: &Arc<Mutex<ConversationManager>>,
    supplementary_system_prompt: Option<String>,
) {
    let client = {
        let manager = conversation.lock().await;
        manager.client()
    };
    client.set_supplementary_system_prompt(supplementary_system_prompt);
}

fn normalize_turn_tokens(input: &str, response: &str, turn_tokens: TurnTokens) -> TurnTokens {
    if turn_tokens.is_zero() {
        TurnTokens {
            input: estimate_tokens(input),
            output: estimate_tokens(response),
            estimated: true,
        }
    } else {
        turn_tokens
    }
}

fn estimate_token_count(messages: &[crate::types::ApiMessage]) -> usize {
    estimate_char_count(messages) / 4
}

fn estimate_char_count(messages: &[crate::types::ApiMessage]) -> usize {
    messages
        .iter()
        .map(|message| match &message.content {
            Content::Text(text) => message.role.len() + text.len(),
            Content::Blocks(blocks) => {
                message.role.len()
                    + blocks
                        .iter()
                        .map(|block| match block {
                            ContentBlock::Text { text } => text.len(),
                            ContentBlock::ToolUse { id, name, input } => {
                                id.len() + name.len() + input.to_string().len()
                            }
                            ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                ..
                            } => tool_use_id.len() + content.len(),
                        })
                        .sum::<usize>()
            }
        })
        .sum()
}

fn forward_conversation_update(
    update: ConversationStreamUpdate,
    textual_block_by_index: &mut std::collections::HashMap<usize, bool>,
    tx: &mpsc::UnboundedSender<UiUpdate>,
) {
    match update {
        ConversationStreamUpdate::Delta(text) => {
            let _ = tx.send(UiUpdate::StreamDelta(text));
        }
        ConversationStreamUpdate::BlockStart { index, block } => {
            let is_textual = matches!(
                block,
                StreamBlock::Thinking { .. } | StreamBlock::FinalText { .. }
            );
            textual_block_by_index.insert(index, is_textual);
            if let StreamBlock::FinalText { content } = &block {
                if !content.is_empty() {
                    let _ = tx.send(UiUpdate::StreamDelta(content.clone()));
                }
            }
            let _ = tx.send(UiUpdate::StreamBlockStart { index, block });
        }
        ConversationStreamUpdate::BlockDelta { index, delta } => {
            let _ = tx.send(UiUpdate::StreamBlockDelta {
                index,
                delta: delta.clone(),
            });
            if textual_block_by_index.get(&index).copied().unwrap_or(false) {
                let _ = tx.send(UiUpdate::StreamDelta(delta));
            }
        }
        ConversationStreamUpdate::BlockComplete { index } => {
            textual_block_by_index.remove(&index);
            let _ = tx.send(UiUpdate::StreamBlockComplete { index });
        }
        ConversationStreamUpdate::ToolApprovalRequest(request) => {
            let _ = tx.send(UiUpdate::ToolApprovalRequest(request));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{estimate_token_count, forward_conversation_update, RuntimeContext};
    use crate::api::{mock_client::MockApiClient, ApiClient};
    use crate::prompts::CODER_SYSTEM_PROMPT;
    use crate::runtime::{EditLoop, EditLoopOutcome, UiUpdate};
    use crate::state::{ConversationManager, ConversationStreamUpdate, StreamBlock};
    use crate::types::{ApiMessage, Content, ContentBlock};
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn test_ref_04_start_turn_dispatches_message() {
        let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();

        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n".to_string(),
            "data: {\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}]}\n\n".to_string(),
        ]])));
        let conversation = ConversationManager::new_mock(client, HashMap::new());

        let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

        ctx.start_turn("test input".to_string());

        let mut saw_delta = false;
        let mut saw_complete = false;
        loop {
            match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
                Ok(Some(UiUpdate::StreamDelta(_))) => saw_delta = true,
                Ok(Some(UiUpdate::TurnComplete)) => {
                    saw_complete = true;
                    break;
                }
                Ok(Some(UiUpdate::Error(e))) => panic!("unexpected error: {e}"),
                Ok(None) | Err(_) => break,
                _ => {}
            }
        }

        assert!(saw_delta, "expected at least one StreamDelta");
        assert!(saw_complete, "expected TurnComplete");
    }

    #[test]
    fn test_ref_07_no_runtime_guard() {
        let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation = ConversationManager::new_mock(client, HashMap::new());
        let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

        ctx.start_turn("test".to_string());

        let update = rx.try_recv().expect("expected error update");
        match update {
            UiUpdate::Error(msg) => {
                assert!(
                    msg.contains("requires active Tokio runtime"),
                    "unexpected error message: {msg}"
                );
            }
            _ => panic!("expected UiUpdate::Error, got something else"),
        }

        assert_eq!(
            ctx.test_message_count_try_lock(),
            Some(0),
            "history must stay clean when guard fires"
        );
    }

    #[tokio::test]
    async fn test_start_edit_loop_dispatches_loop_completion() {
        let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation = ConversationManager::new_mock(client, HashMap::new());
        let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

        ctx.start_edit_loop(
            EditLoop::new("task-edit-loop".to_string()).with_max_turns(1),
            "fix the parser".to_string(),
        );

        loop {
            match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
                Ok(Some(UiUpdate::TranscriptLine(_))) => {}
                Ok(Some(UiUpdate::EditLoopComplete { outcome, .. })) => {
                    assert!(matches!(outcome, EditLoopOutcome::MaxTurnsReached { .. }));
                    return;
                }
                Ok(Some(UiUpdate::Error(e))) => panic!("unexpected error: {e}"),
                Ok(None) | Err(_) => panic!("expected EditLoopComplete"),
                _ => {}
            }
        }
    }

    #[tokio::test]
    async fn test_coding_prompt_injected_during_edit_loop_only() {
        let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation = ConversationManager::new_mock(client, HashMap::new());
        let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

        assert!(
            !ctx.test_system_prompt()
                .await
                .contains(CODER_SYSTEM_PROMPT.trim()),
            "base prompt must not include the coding prompt before edit loop activation"
        );

        ctx.start_edit_loop(
            EditLoop::new("task-edit-loop-prompt".to_string()).with_max_turns(128),
            "fix the parser".to_string(),
        );

        tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                if ctx
                    .test_system_prompt()
                    .await
                    .contains(CODER_SYSTEM_PROMPT.trim())
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("coding prompt must be injected while the edit loop is active");

        loop {
            match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
                Ok(Some(UiUpdate::EditLoopComplete { .. })) => break,
                Ok(Some(UiUpdate::TranscriptLine(_))) => {}
                Ok(Some(UiUpdate::Error(e))) => panic!("unexpected error: {e}"),
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => panic!("expected EditLoopComplete"),
            }
        }

        assert!(
            !ctx.test_system_prompt()
                .await
                .contains(CODER_SYSTEM_PROMPT.trim()),
            "coding prompt must clear after the edit loop completes"
        );
    }

    #[test]
    fn test_estimated_token_count_uses_chars_div_4() {
        let messages = vec![
            ApiMessage {
                role: "user".to_string(),
                content: Content::Text("abcd".to_string()),
            },
            ApiMessage {
                role: "assistant".to_string(),
                content: Content::Blocks(vec![
                    ContentBlock::Text {
                        text: "efgh".to_string(),
                    },
                    ContentBlock::ToolResult {
                        tool_use_id: "tool-1".to_string(),
                        content: "ijkl".to_string(),
                        is_error: false,
                    },
                ]),
            },
        ];

        assert_eq!(estimate_token_count(&messages), 7);
    }

    #[tokio::test]
    async fn test_ref_08_start_turn_full_protocol_parity() {
        let chunks = vec![vec![
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n".to_string(),
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n".to_string(),
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n\n".to_string(),
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n".to_string(),
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
        ]];

        let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(chunks)));
        let conversation = ConversationManager::new_mock(client, HashMap::new());
        let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

        ctx.start_turn("test".to_string());

        let mut events: Vec<&str> = vec![];
        loop {
            match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
                Ok(Some(UiUpdate::StreamBlockStart { .. })) => events.push("BlockStart"),
                Ok(Some(UiUpdate::StreamBlockDelta { .. })) => events.push("BlockDelta"),
                Ok(Some(UiUpdate::StreamBlockComplete { .. })) => events.push("BlockComplete"),
                Ok(Some(UiUpdate::StreamDelta(_))) => events.push("Delta"),
                Ok(Some(UiUpdate::ToolApprovalRequest(_))) => events.push("ToolApproval"),
                Ok(Some(UiUpdate::TurnComplete)) => {
                    events.push("TurnComplete");
                    break;
                }
                Ok(Some(UiUpdate::Error(e))) => panic!("unexpected error: {e}"),
                _ => break,
            }
        }

        assert!(
            events.contains(&"TurnComplete"),
            "must terminate with TurnComplete"
        );
        assert_eq!(
            events.iter().filter(|&&e| e == "TurnComplete").count(),
            1,
            "exactly one TurnComplete"
        );
    }
    #[tokio::test(flavor = "current_thread")]
    async fn test_ref_08_tool_approval_forwarding_no_hang() {
        let _env_lock = crate::test_support::ENV_LOCK.lock().await;
        std::env::set_var("VEX_TOOL_CONFIRM", "true");
        let first_response_sse = vec![
            r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_tool_then_final_1","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
            r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
            r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_mock_round_1","name":"read_file","input":{}}}"#.to_string(),
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"file.txt\"}"}}"#.to_string(),
            r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#.to_string(),
            r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":10}}"#.to_string(),
            r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
        ];

        let second_response_sse = vec![
            r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_tool_then_final_2","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#.to_string(),
            r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"done"}}"#.to_string(),
            r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":7}}"#.to_string(),
            r#"event: message_stop
data: {"type":"message_stop"}"#.to_string(),
        ];

        let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![
            first_response_sse,
            second_response_sse,
        ])));
        let conversation = ConversationManager::new_mock(client, HashMap::new());
        let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

        ctx.start_turn("read file".to_string());

        let mut saw_request = false;
        let mut saw_complete = false;
        loop {
            match tokio::time::timeout(Duration::from_millis(800), rx.recv()).await {
                Ok(Some(UiUpdate::ToolApprovalRequest(request))) => {
                    saw_request = true;
                    let _ = request.response_tx.send(false);
                }
                Ok(Some(UiUpdate::TurnComplete)) => {
                    saw_complete = true;
                    break;
                }
                Ok(Some(UiUpdate::Error(e))) => panic!("unexpected error: {e}"),
                Ok(Some(_)) => {}
                _ => break,
            }
        }

        assert!(saw_request, "must forward tool approval request");
        assert!(saw_complete, "must finish turn after approval response");
        std::env::remove_var("VEX_TOOL_CONFIRM");
    }

    #[tokio::test]
    async fn test_ref_08_block_delta_partial_json_not_mirrored_to_stream_delta() {
        let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
        let mut textual_block_by_index = std::collections::HashMap::new();

        forward_conversation_update(
            ConversationStreamUpdate::BlockStart {
                index: 1,
                block: StreamBlock::ToolCall {
                    id: "toolu_1".to_string(),
                    name: "read_file".to_string(),
                    input: serde_json::json!({}),
                    status: crate::state::ToolStatus::Pending,
                },
            },
            &mut textual_block_by_index,
            &tx,
        );

        forward_conversation_update(
            ConversationStreamUpdate::BlockDelta {
                index: 1,
                delta: "{\"path\":\"file.txt\"}".to_string(),
            },
            &mut textual_block_by_index,
            &tx,
        );

        let mut saw_block_delta = false;
        let mut leaked_stream_delta = false;
        for _ in 0..4 {
            match rx.try_recv() {
                Ok(UiUpdate::StreamBlockDelta { delta, .. }) if delta.contains("path") => {
                    saw_block_delta = true
                }
                Ok(UiUpdate::StreamDelta(text)) if text.contains("path") => {
                    leaked_stream_delta = true
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        assert!(
            saw_block_delta,
            "expected StreamBlockDelta from partial_json"
        );
        assert!(
            !leaked_stream_delta,
            "partial_json must not leak into StreamDelta"
        );
    }

    #[tokio::test]
    async fn test_ref_08_unknown_block_index_delta_does_not_mirror_to_stream_delta() {
        let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
        let mut textual_block_by_index = std::collections::HashMap::new();

        forward_conversation_update(
            ConversationStreamUpdate::BlockDelta {
                index: 99,
                delta: "mystery".to_string(),
            },
            &mut textual_block_by_index,
            &tx,
        );

        let mut saw_block_delta = false;
        let mut saw_stream_delta = false;
        while let Ok(update) = rx.try_recv() {
            match update {
                UiUpdate::StreamBlockDelta { .. } => saw_block_delta = true,
                UiUpdate::StreamDelta(_) => saw_stream_delta = true,
                _ => {}
            }
        }

        assert!(saw_block_delta, "block delta should always be forwarded");
        assert!(
            !saw_stream_delta,
            "unknown block index must not mirror into StreamDelta"
        );
    }

    #[tokio::test]
    async fn test_ref_08_cancel_turn_resets_root_token_for_next_turn() {
        let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![vec![
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n".to_string(),
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n".to_string(),
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n".to_string(),
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
        ]])));
        let conversation = ConversationManager::new_mock(client, HashMap::new());
        let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

        assert!(!ctx.test_root_cancelled());
        ctx.cancel_turn();
        assert!(
            !ctx.test_root_cancelled(),
            "cancel_turn must replace root token with a fresh non-cancelled token"
        );

        ctx.start_turn("turn B".to_string());

        let progressed = tokio::time::timeout(Duration::from_millis(800), async {
            loop {
                match rx.recv().await {
                    Some(UiUpdate::StreamDelta(_) | UiUpdate::TurnComplete) => return true,
                    Some(UiUpdate::Error(_)) | None => return false,
                    Some(_) => {}
                }
            }
        })
        .await
        .unwrap_or(false);

        assert!(
            progressed,
            "turn after cancel_turn must emit at least one normal update with fresh root token"
        );
    }

    #[tokio::test]
    async fn test_ref_08_cancel_path_emits_single_terminal_event() {
        let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n".to_string(),
        ]])));
        let conversation = ConversationManager::new_mock(client, HashMap::new());
        let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());

        ctx.start_turn("test".to_string());
        ctx.cancel_turn();

        let mut terminal_count = 0;
        for _ in 0..6 {
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Some(UiUpdate::TurnComplete | UiUpdate::Error(_))) => terminal_count += 1,
                Ok(Some(_)) => {}
                _ => break,
            }
        }

        assert_eq!(
            terminal_count, 1,
            "cancel path must emit exactly one terminal event"
        );
    }
}
