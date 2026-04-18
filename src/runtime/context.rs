use crate::runtime::{EditLoop, UiUpdate};
use crate::state::{ConversationManager, ConversationStreamUpdate, StreamBlock, TurnToolPolicy};
use crate::types::{Content, ContentBlock};
use crate::usage::{SessionTokens, TurnTokens, estimate_tokens};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

pub struct RuntimeContext {
    conversation: Arc<Mutex<ConversationManager>>,
    update_tx: mpsc::UnboundedSender<UiUpdate>,
    cancel: CancellationToken,
    session_tokens: Arc<StdMutex<SessionTokens>>,
}

pub(crate) struct EditTurnResult {
    pub patch_applied: bool,
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

            let mut textual_block_by_index =
                std::collections::HashMap::<usize, crate::api::stream::StreamTextNormaliser>::new();
            let mut normaliser = crate::api::stream::StreamTextNormaliser::new();
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
                            Some(update) => forward_conversation_update(update, &mut textual_block_by_index, &mut normaliser, &tx),
                            None => break,
                        }
                    }
                }
            }

            // Flush normaliser buffers when the stream ends naturally.
            // This emits any pending text and closes stale open tool-call
            // blocks so the TUI sees a clean terminal state. Skip when
            // cancelled – the turn was aborted and partial output is noise.
            if !cancelled {
                flush_normalised_text(&mut normaliser, &tx);
                for (index, mut block_normaliser) in textual_block_by_index.drain() {
                    flush_normalised_block_text(&mut block_normaliser, index, &tx);
                    let _ = tx.send(UiUpdate::StreamBlockComplete { index });
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
        let system_prompt = match edit_loop.profile.system_prompt_text() {
            Ok(text) => text.to_string(),
            Err(e) => {
                let _ = self
                    .update_tx
                    .send(UiUpdate::Error(format!("edit loop profile error: {e}")));
                return;
            }
        };

        set_runtime_prompt_now(&self.conversation, Some(system_prompt.clone()));

        tokio::spawn(async move {
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

    /// Drive a single model turn within the current async task and wait for
    /// completion, forwarding stream events to the TUI while the turn runs.
    ///
    /// Used by `EditLoop::run` to execute assemble→model→apply cycles without
    /// spawning a detached task. The conversation lock is held only for the
    /// duration of `send_message_with_policy`.
    pub(crate) async fn drive_edit_turn(&self, input: String) -> anyhow::Result<EditTurnResult> {
        let (delta_tx, mut delta_rx) = mpsc::unbounded_channel::<ConversationStreamUpdate>();
        let conversation = Arc::clone(&self.conversation);
        let tx = self.update_tx.clone();

        let send_handle = tokio::spawn(async move {
            let mut mgr = conversation.lock().await;
            let result = mgr
                .send_message_with_policy(input, Some(&delta_tx), TurnToolPolicy::Default)
                .await;
            let patch_applied = mgr.current_turn_has_successful_mutation();
            (result, patch_applied)
        });

        let mut textual_block_by_index =
            std::collections::HashMap::<usize, crate::api::stream::StreamTextNormaliser>::new();
        let mut normaliser = crate::api::stream::StreamTextNormaliser::new();
        while let Some(update) = delta_rx.recv().await {
            forward_conversation_update(update, &mut textual_block_by_index, &mut normaliser, &tx);
        }
        // Flush normaliser state after the edit stream ends so that any
        // buffered text and stale open tool-call blocks are emitted cleanly.
        flush_normalised_text(&mut normaliser, &tx);
        for (index, mut block_normaliser) in textual_block_by_index.drain() {
            flush_normalised_block_text(&mut block_normaliser, index, &tx);
            let _ = tx.send(UiUpdate::StreamBlockComplete { index });
        }

        match send_handle.await {
            Ok((Ok(_response_text), patch_applied)) => Ok(EditTurnResult { patch_applied }),
            Ok((Err(e), _)) => Err(e),
            Err(e) => Err(anyhow::anyhow!("edit turn task failed: {e}")),
        }
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

    pub async fn shutdown_resources(&mut self) {
        self.cancel.cancel();
        let mut conversation = self.conversation.lock().await;
        conversation.shutdown_resources().await;
        self.cancel = CancellationToken::new();
    }

    pub fn turn_cancellation_token(&self) -> CancellationToken {
        self.cancel.child_token()
    }

    pub fn emit_turn_complete(&self) {
        let _ = self.update_tx.send(UiUpdate::TurnComplete);
    }

    pub fn emit_command_session_attached(&self, session_id: u64, pid: Option<u32>) {
        let _ = self
            .update_tx
            .send(UiUpdate::CommandSessionAttached { session_id, pid });
    }

    pub fn emit_command_session_finished(&self, session_id: u64) {
        let _ = self
            .update_tx
            .send(UiUpdate::CommandSessionFinished { session_id });
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

    pub fn pop_undo_checkpoint(&self) -> Option<crate::state::UndoCheckpoint> {
        if let Ok(mut conversation) = self.conversation.try_lock() {
            return conversation.pop_undo_checkpoint();
        }
        self.conversation.blocking_lock().pop_undo_checkpoint()
    }

    pub fn undo_stack_len(&self) -> usize {
        if let Ok(conversation) = self.conversation.try_lock() {
            return conversation.undo_stack_len();
        }
        self.conversation.blocking_lock().undo_stack_len()
    }

    pub fn is_undo_enabled(&self) -> bool {
        if let Ok(conversation) = self.conversation.try_lock() {
            return conversation.is_undo_enabled();
        }
        self.conversation.blocking_lock().is_undo_enabled()
    }

    pub fn emit_transcript_line(&self, line: String) {
        let _ = self.update_tx.send(UiUpdate::TranscriptLine(line));
    }

    pub fn session_tokens_rollup(&self) -> SessionTokens {
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

    /// Poll the configured local inference server for capabilities and cache
    /// the result on the shared `ApiClient`. No-op for remote endpoints or
    /// when the server does not expose a discovery endpoint.
    pub async fn populate_local_server_info(&self) {
        let client = {
            let manager = self.conversation.lock().await;
            manager.client()
        };
        client.populate_server_info().await;
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

fn set_runtime_prompt_now(
    conversation: &Arc<Mutex<ConversationManager>>,
    supplementary_system_prompt: Option<String>,
) {
    if let Ok(manager) = conversation.try_lock() {
        manager
            .client()
            .set_supplementary_system_prompt(supplementary_system_prompt);
    }
}

fn normalize_turn_tokens(input: &str, response: &str, turn_tokens: TurnTokens) -> TurnTokens {
    if turn_tokens.is_zero() {
        TurnTokens {
            input: estimate_tokens(input),
            output: estimate_tokens(response),
            estimated: true,
            ..Default::default()
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
                            ContentBlock::Text { text, .. } => text.len(),
                            ContentBlock::ToolUse {
                                id, name, input, ..
                            } => id.len() + name.len() + input.to_string().len(),
                            ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                ..
                            } => tool_use_id.len() + content.len(),
                            ContentBlock::Thinking {
                                thinking,
                                signature,
                            } => thinking.len() + signature.len(),
                            ContentBlock::SuppressedThinking { data } => data.len(),
                            ContentBlock::ServerToolUse { id, name, input } => {
                                id.len() + name.len() + input.to_string().len()
                            }
                            ContentBlock::WebSearchToolResult {
                                tool_use_id,
                                content,
                            } => tool_use_id.len() + content.to_string().len(),
                        })
                        .sum::<usize>()
            }
        })
        .sum()
}

fn forward_conversation_update(
    update: ConversationStreamUpdate,
    textual_block_by_index: &mut std::collections::HashMap<
        usize,
        crate::api::stream::StreamTextNormaliser,
    >,
    normaliser: &mut crate::api::stream::StreamTextNormaliser,
    tx: &mpsc::UnboundedSender<UiUpdate>,
) {
    match update {
        ConversationStreamUpdate::Delta(text) => {
            emit_normalised_text(normaliser, &text, tx);
        }
        ConversationStreamUpdate::BlockStart { index, block } => match block {
            StreamBlock::Thinking { content, collapsed } => {
                flush_normalised_text(normaliser, tx);
                if let Some(existing) = textual_block_by_index.get_mut(&index) {
                    flush_normalised_block_text(existing, index, tx);
                }
                textual_block_by_index
                    .insert(index, crate::api::stream::StreamTextNormaliser::new());
                let _ = tx.send(UiUpdate::StreamBlockStart {
                    index,
                    block: StreamBlock::Thinking {
                        content: String::new(),
                        collapsed,
                    },
                });
                if !content.is_empty()
                    && let Some(block_normaliser) = textual_block_by_index.get_mut(&index)
                {
                    emit_normalised_block_text(block_normaliser, index, &content, tx);
                }
            }
            StreamBlock::FinalText { content } => {
                flush_normalised_text(normaliser, tx);
                if let Some(existing) = textual_block_by_index.get_mut(&index) {
                    flush_normalised_block_text(existing, index, tx);
                }
                textual_block_by_index
                    .insert(index, crate::api::stream::StreamTextNormaliser::new());
                let _ = tx.send(UiUpdate::StreamBlockStart {
                    index,
                    block: StreamBlock::FinalText {
                        content: String::new(),
                    },
                });
                if !content.is_empty()
                    && let Some(block_normaliser) = textual_block_by_index.get_mut(&index)
                {
                    emit_normalised_block_text(block_normaliser, index, &content, tx);
                }
            }
            other => {
                let _ = tx.send(UiUpdate::StreamBlockStart {
                    index,
                    block: other,
                });
            }
        },
        ConversationStreamUpdate::BlockDelta { index, delta } => {
            if let Some(block_normaliser) = textual_block_by_index.get_mut(&index) {
                emit_normalised_block_text(block_normaliser, index, &delta, tx);
            } else {
                let _ = tx.send(UiUpdate::StreamBlockDelta { index, delta });
            }
        }
        ConversationStreamUpdate::BlockComplete { index } => {
            if let Some(mut block_normaliser) = textual_block_by_index.remove(&index) {
                flush_normalised_block_text(&mut block_normaliser, index, tx);
            }
            let _ = tx.send(UiUpdate::StreamBlockComplete { index });
        }
        ConversationStreamUpdate::ToolApprovalRequest(request) => {
            let _ = tx.send(UiUpdate::ToolApprovalRequest(request));
        }
        ConversationStreamUpdate::TranscriptLine(line) => {
            let _ = tx.send(UiUpdate::TranscriptLine(line));
        }
        ConversationStreamUpdate::ServerMetadata(metadata) => {
            let _ = tx.send(UiUpdate::ServerMetadata(metadata));
        }
        ConversationStreamUpdate::CommandSessionStarted {
            session_id,
            command,
        } => {
            let _ = tx.send(UiUpdate::CommandSessionStarted {
                session_id,
                command,
            });
        }
        ConversationStreamUpdate::CommandSessionAttached { session_id, pid } => {
            let _ = tx.send(UiUpdate::CommandSessionAttached { session_id, pid });
        }
        ConversationStreamUpdate::CommandSessionFinished { session_id } => {
            let _ = tx.send(UiUpdate::CommandSessionFinished { session_id });
        }
        ConversationStreamUpdate::ContextCompacted {
            messages_before,
            messages_after,
            summary,
        } => {
            let _ = tx.send(UiUpdate::ContextCompacted {
                messages_before,
                messages_after,
                summary,
            });
        }
        // Surface stream errors (API errors, SSE parse failures) to the UI.
        // ADR-021 Item 19.
        ConversationStreamUpdate::StreamError(msg) => {
            let _ = tx.send(UiUpdate::Error(msg));
        }
    }
}

/// Route standalone text through the normaliser before sending it to the UI.
///
/// Embedded tool call markup is intercepted and converted to structured
/// `TranscriptLine` entries. Clean text passes through as `StreamDelta` for
/// unindexed updates and `StreamBlockDelta` for textual block streams. This is
/// the single authoritative boundary between streamed model text and the TUI's
/// task-state projection.
fn emit_normalised_text(
    normaliser: &mut crate::api::stream::StreamTextNormaliser,
    text: &str,
    tx: &mpsc::UnboundedSender<UiUpdate>,
) {
    emit_normalised_chunks(normaliser.normalise(text), None, tx);
}

fn flush_normalised_text(
    normaliser: &mut crate::api::stream::StreamTextNormaliser,
    tx: &mpsc::UnboundedSender<UiUpdate>,
) {
    emit_normalised_chunks(normaliser.flush(), None, tx);
}

fn emit_normalised_block_text(
    normaliser: &mut crate::api::stream::StreamTextNormaliser,
    index: usize,
    text: &str,
    tx: &mpsc::UnboundedSender<UiUpdate>,
) {
    emit_normalised_chunks(normaliser.normalise(text), Some(index), tx);
}

fn flush_normalised_block_text(
    normaliser: &mut crate::api::stream::StreamTextNormaliser,
    index: usize,
    tx: &mpsc::UnboundedSender<UiUpdate>,
) {
    emit_normalised_chunks(normaliser.flush(), Some(index), tx);
}

fn emit_normalised_chunks(
    chunks: Vec<crate::api::stream::NormalisedChunk>,
    block_index: Option<usize>,
    tx: &mpsc::UnboundedSender<UiUpdate>,
) {
    use crate::api::stream::NormalisedChunk;
    for chunk in chunks {
        match chunk {
            NormalisedChunk::Text(text) => {
                if let Some(index) = block_index {
                    let _ = tx.send(UiUpdate::StreamBlockDelta { index, delta: text });
                } else {
                    let _ = tx.send(UiUpdate::StreamDelta(text));
                }
            }
            NormalisedChunk::TranscriptLine(line) => {
                let _ = tx.send(UiUpdate::TranscriptLine(line));
            }
        }
    }
}

#[cfg(test)]
mod tests;
