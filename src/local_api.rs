//! LocalApiServer runtime mode implementation.
//!
//! This module contains the `LocalApiMode` (RuntimeMode) and
//! `LocalApiFrontend` (FrontendAdapter) implementations that bridge
//! the ADR-026 local API surface to the runtime engine.
//!
//! Transport wiring (HTTP routing, TLS, SSE framing, Unix sockets)
//! is found in `crate::server` per ADR-028.

#[cfg(test)]
use crate::api::ApiClient;
use crate::app::FacadeSessionTaskRollup;
use crate::config::Config;
use crate::runtime::UiUpdate;
use crate::runtime::context::RuntimeContext;
use crate::runtime::delta_accumulator::{DeltaAccumulator, PeerDeltaEvent};
use crate::runtime::frontend::{FrontendAdapter, UserInputEvent};
use crate::runtime::json_handoff::{
    RuntimeEnvelope, RuntimeEnvelopeNormalizer, RuntimeEvent, TurnEndContext,
    runtime_approval_request_event,
};
use crate::runtime::mode::RuntimeMode;
use crate::runtime::tokio::sync::{Mutex as AsyncMutex, broadcast, mpsc, oneshot};
#[cfg(test)]
use crate::state::ConversationManager;
use crate::state::TurnToolPolicy;
#[cfg(test)]
use crate::tools::ToolOperator;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const SESSION_TASK_EVENT_BUFFER: usize = 64;
const PEER_EVENT_BUFFER: usize = 128;
const RECENT_PEER_EVENT_LIMIT: usize = 128;

#[derive(Clone)]
pub(crate) struct LocalApiState {
    pub config: Config,
    pub tasks: Arc<AsyncMutex<HashMap<String, ActiveTask>>>,
    session_task_events: broadcast::Sender<FacadeSessionTaskRollup>,
}

pub(crate) struct ActiveTask {
    pub interrupt_tx: mpsc::UnboundedSender<FrontendCommand>,
    pub shared: Arc<Mutex<LocalApiTaskShared>>,
}

pub(crate) struct LocalApiTaskShared {
    pub normalizer: RuntimeEnvelopeNormalizer,
    pub envelope_tx: mpsc::UnboundedSender<String>,
    pub peer_event_rx: mpsc::Receiver<PeerDeltaEvent>,
    pub recent_peer_events: VecDeque<PeerDeltaEvent>,
    pub pending_approval: Option<PendingApproval>,
    pub quit: Arc<AtomicBool>,
    pub turn_in_progress: bool,
    pub interrupted: bool,
    pub active_command_sessions: BTreeSet<u64>,
    pub turn_completion_pending: bool,
}

impl LocalApiTaskShared {
    /// Constructs a new `LocalApiTaskShared` with a fresh delta accumulator.
    ///
    /// `memory_watermark_bytes` should be read from
    /// [`crate::config::ApiClientConfig::delta_accumulator_memory_watermark_bytes`]
    /// by the server handler.  Pass
    /// `crate::runtime::delta_accumulator::DEFAULT_DELTA_ACCUMULATOR_MEMORY_WATERMARK_BYTES`
    /// from paths that do not have access to the full config.
    pub fn new(
        task_id: String,
        envelope_tx: mpsc::UnboundedSender<String>,
        quit: Arc<AtomicBool>,
        memory_watermark_bytes: usize,
    ) -> Self {
        let (peer_event_tx, peer_event_rx) = mpsc::channel(PEER_EVENT_BUFFER);
        let delta_accumulator = Arc::new(DeltaAccumulator::new_with_peer_events(
            memory_watermark_bytes,
            Some(peer_event_tx),
        ));

        Self {
            normalizer: RuntimeEnvelopeNormalizer::new_with_delta_accumulator(
                task_id,
                delta_accumulator,
            ),
            envelope_tx,
            peer_event_rx,
            recent_peer_events: VecDeque::new(),
            pending_approval: None,
            quit,
            turn_in_progress: false,
            interrupted: false,
            active_command_sessions: BTreeSet::new(),
            turn_completion_pending: false,
        }
    }

    fn drain_peer_events(&mut self) {
        while let Ok(event) = self.peer_event_rx.try_recv() {
            if self.recent_peer_events.len() == RECENT_PEER_EVENT_LIMIT {
                self.recent_peer_events.pop_front();
            }
            self.recent_peer_events.push_back(event);
        }
    }
}

pub(crate) struct PendingApproval {
    pub capability: String,
    pub scope: String,
    pub response_tx: oneshot::Sender<bool>,
}

pub(crate) enum FrontendCommand {
    Interrupt,
}

pub(crate) struct LocalApiFrontend {
    initial_input: Option<String>,
    command_rx: mpsc::UnboundedReceiver<FrontendCommand>,
    quit: Arc<AtomicBool>,
}

impl LocalApiFrontend {
    pub fn new(
        initial_input: String,
        command_rx: mpsc::UnboundedReceiver<FrontendCommand>,
        quit: Arc<AtomicBool>,
    ) -> Self {
        Self {
            initial_input: Some(initial_input),
            command_rx,
            quit,
        }
    }

    pub fn should_quit(&self) -> bool {
        self.quit.load(Ordering::SeqCst)
    }
}

impl FrontendAdapter<LocalApiMode> for LocalApiFrontend {
    fn poll_user_input(&mut self, mode: &LocalApiMode) -> Option<UserInputEvent> {
        if let Ok(command) = self.command_rx.try_recv() {
            return match command {
                FrontendCommand::Interrupt => Some(UserInputEvent::Interrupt),
            };
        }

        if !mode.is_turn_in_progress() {
            return self.initial_input.take().map(UserInputEvent::Text);
        }

        None
    }

    fn render(&mut self, _mode: &LocalApiMode) {}

    fn should_quit(&self) -> bool {
        self.quit.load(Ordering::SeqCst)
    }
}

pub(crate) struct LocalApiMode {
    shared: Arc<Mutex<LocalApiTaskShared>>,
}

impl LocalApiMode {
    pub fn new(shared: Arc<Mutex<LocalApiTaskShared>>) -> Self {
        Self { shared }
    }

    pub fn emit_envelopes(shared: &mut LocalApiTaskShared, envelopes: Vec<RuntimeEnvelope>) {
        shared.drain_peer_events();
        for envelope in envelopes {
            if let Ok(json) = serde_json::to_string(&envelope) {
                let _ = shared.envelope_tx.send(json);
            }
        }
        shared.drain_peer_events();
    }

    fn complete_turn_if_idle(shared: &mut LocalApiTaskShared) {
        if !shared.turn_completion_pending || !shared.active_command_sessions.is_empty() {
            return;
        }

        let envelopes = if shared.interrupted {
            shared.normalizer.emit_cancelled(TurnEndContext::default())
        } else {
            shared
                .normalizer
                .normalize_ui_update(&UiUpdate::TurnComplete, Some(TurnEndContext::default()))
        };
        shared.turn_completion_pending = false;
        shared.turn_in_progress = false;
        shared.quit.store(true, Ordering::SeqCst);
        Self::emit_envelopes(shared, envelopes);
    }
}

impl RuntimeMode for LocalApiMode {
    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext) {
        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        shared.turn_in_progress = true;
        shared.interrupted = false;
        shared.active_command_sessions.clear();
        shared.turn_completion_pending = false;
        shared.quit.store(false, Ordering::SeqCst);
        let start = shared.normalizer.start_turn(1, Some(input.clone()));
        Self::emit_envelopes(&mut shared, vec![start]);
        drop(shared);

        ctx.start_turn_with_system_prompt_and_policy(input, None, TurnToolPolicy::Default);
    }

    fn on_interrupt(&mut self, ctx: &mut RuntimeContext) {
        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if !shared.turn_in_progress {
            return;
        }
        shared.interrupted = true;
        drop(shared);
        ctx.cancel_turn();
    }

    fn on_model_update(&mut self, update: UiUpdate, _ctx: &mut RuntimeContext) {
        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        match update {
            UiUpdate::TranscriptLine(line) => {
                let envelopes = shared
                    .normalizer
                    .normalize_ui_update(&UiUpdate::TranscriptLine(line), None);
                Self::emit_envelopes(&mut shared, envelopes);
            }
            UiUpdate::StreamDelta(text) => {
                let envelopes = shared
                    .normalizer
                    .normalize_ui_update(&UiUpdate::StreamDelta(text), None);
                Self::emit_envelopes(&mut shared, envelopes);
            }
            UiUpdate::StreamBlockStart { index, block } => {
                let envelopes = shared
                    .normalizer
                    .normalize_ui_update(&UiUpdate::StreamBlockStart { index, block }, None);
                Self::emit_envelopes(&mut shared, envelopes);
            }
            UiUpdate::StreamBlockDelta { index, delta } => {
                let envelopes = shared
                    .normalizer
                    .normalize_ui_update(&UiUpdate::StreamBlockDelta { index, delta }, None);
                Self::emit_envelopes(&mut shared, envelopes);
            }
            UiUpdate::StreamBlockComplete { index } => {
                let envelopes = shared
                    .normalizer
                    .normalize_ui_update(&UiUpdate::StreamBlockComplete { index }, None);
                Self::emit_envelopes(&mut shared, envelopes);
            }
            UiUpdate::ToolApprovalRequest(request) => {
                let event = runtime_approval_request_event(&request);
                let (capability, scope) = match &event {
                    RuntimeEvent::ApprovalRequest {
                        capability, scope, ..
                    } => (capability.clone(), scope.clone()),
                    _ => ("unknown".to_string(), "once".to_string()),
                };
                shared.pending_approval = Some(PendingApproval {
                    capability,
                    scope,
                    response_tx: request.response_tx,
                });
                let envelope = shared.normalizer.emit_event(event);
                Self::emit_envelopes(&mut shared, vec![envelope]);
            }
            UiUpdate::ServerMetadata(_) => {}
            UiUpdate::TurnComplete => {
                shared.turn_completion_pending = true;
                Self::complete_turn_if_idle(&mut shared);
            }
            UiUpdate::Error(message) => {
                let envelopes = shared.normalizer.emit_error(
                    "runtime_error".to_string(),
                    message,
                    false,
                    TurnEndContext::default(),
                );
                shared.active_command_sessions.clear();
                shared.turn_completion_pending = false;
                shared.turn_in_progress = false;
                shared.quit.store(true, Ordering::SeqCst);
                Self::emit_envelopes(&mut shared, envelopes);
            }
            UiUpdate::CommandSessionStarted { session_id, .. } => {
                shared.active_command_sessions.insert(session_id);
            }
            UiUpdate::CommandSessionAttached { .. }
            | UiUpdate::EditLoopComplete { .. }
            | UiUpdate::ContextCompacted { .. } => {}
            UiUpdate::CommandSessionFinished { session_id } => {
                shared.active_command_sessions.remove(&session_id);
                Self::complete_turn_if_idle(&mut shared);
            }
        }
    }

    fn is_turn_in_progress(&self) -> bool {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .turn_in_progress
    }
}

impl LocalApiState {
    pub fn new(config: Config) -> Self {
        let (session_task_events, _) = broadcast::channel(SESSION_TASK_EVENT_BUFFER);
        Self {
            config,
            tasks: Arc::new(AsyncMutex::new(HashMap::new())),
            session_task_events,
        }
    }

    pub fn publish_session_task_rollup(&self, snapshot: FacadeSessionTaskRollup) {
        let _ = self.session_task_events.send(snapshot);
    }

    pub fn subscribe_session_task_events(&self) -> broadcast::Receiver<FacadeSessionTaskRollup> {
        self.session_task_events.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::mock_client::MockApiClient;
    use crate::runtime::json_handoff::{RuntimeEnvelope, RuntimeRequest};
    use crate::state::StreamBlock;
    use serde_json::json;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn test_local_api_mode_interrupt_emits_cancelled_turn_end() {
        let (envelope_tx, mut envelope_rx) = mpsc::unbounded_channel();
        let quit = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(LocalApiTaskShared {
            normalizer: RuntimeEnvelopeNormalizer::new("task-1"),
            envelope_tx,
            peer_event_rx: mpsc::channel(1).1,
            recent_peer_events: VecDeque::new(),
            pending_approval: None,
            quit,
            turn_in_progress: false,
            interrupted: false,
            active_command_sessions: BTreeSet::new(),
            turn_completion_pending: false,
        }));
        let mut mode = LocalApiMode::new(Arc::clone(&shared));
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation =
            ConversationManager::new(client, ToolOperator::new(std::env::temp_dir()));
        let (update_tx, _update_rx) = mpsc::unbounded_channel();
        let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());

        mode.on_user_input("review src/app.rs".to_string(), &mut ctx);
        let _ = envelope_rx.recv().await.unwrap();
        mode.on_interrupt(&mut ctx);
        mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

        let final_envelope: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        assert!(matches!(
            final_envelope.event,
            RuntimeEvent::TurnEnd { ref status, .. } if status == "cancelled"
        ));
    }

    #[tokio::test]
    async fn test_local_api_mode_emits_approval_request_and_resolution() {
        let (envelope_tx, mut envelope_rx) = mpsc::unbounded_channel();
        let quit = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(LocalApiTaskShared {
            normalizer: RuntimeEnvelopeNormalizer::new("task-2"),
            envelope_tx,
            peer_event_rx: mpsc::channel(1).1,
            recent_peer_events: VecDeque::new(),
            pending_approval: None,
            quit,
            turn_in_progress: false,
            interrupted: false,
            active_command_sessions: BTreeSet::new(),
            turn_completion_pending: false,
        }));
        let mut mode = LocalApiMode::new(Arc::clone(&shared));
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation =
            ConversationManager::new(client, ToolOperator::new(std::env::temp_dir()));
        let (update_tx, _update_rx) = mpsc::unbounded_channel();
        let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
        let (response_tx, _response_rx) = tokio::sync::oneshot::channel();

        mode.on_user_input("review".to_string(), &mut ctx);
        let _ = envelope_rx.recv().await.unwrap();
        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(crate::state::ToolApprovalRequest {
                tool_name: "run_command".to_string(),
                input_preview: "{}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );

        let request: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        assert!(matches!(
            request.event,
            RuntimeEvent::ApprovalRequest { .. }
        ));

        {
            let mut shared = shared.lock().unwrap();
            let pending = shared.pending_approval.take().unwrap();
            let _ = pending.response_tx.send(true);
            let envelopes =
                shared
                    .normalizer
                    .normalize_runtime_request(&RuntimeRequest::ApproveCapability {
                        request_id: "req-approve-local-api".to_string(),
                        task_id: "task-2".to_string(),
                        capability: pending.capability,
                        scope: pending.scope,
                    });
            LocalApiMode::emit_envelopes(&mut shared, envelopes);
        }

        let resolved: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        assert!(matches!(
            resolved.event,
            RuntimeEvent::ApprovalResolved { approved: true, .. }
        ));
    }

    #[tokio::test]
    async fn test_local_api_mode_emits_stream_sequence_in_order() {
        let (envelope_tx, mut envelope_rx) = mpsc::unbounded_channel();
        let quit = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(LocalApiTaskShared {
            normalizer: RuntimeEnvelopeNormalizer::new("task-seq"),
            envelope_tx,
            peer_event_rx: mpsc::channel(1).1,
            recent_peer_events: VecDeque::new(),
            pending_approval: None,
            quit,
            turn_in_progress: false,
            interrupted: false,
            active_command_sessions: BTreeSet::new(),
            turn_completion_pending: false,
        }));
        let mut mode = LocalApiMode::new(Arc::clone(&shared));
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation =
            ConversationManager::new(client, ToolOperator::new(std::env::temp_dir()));
        let (update_tx, _update_rx) = mpsc::unbounded_channel();
        let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());

        mode.on_user_input("review src/local_api.rs".to_string(), &mut ctx);
        mode.on_model_update(UiUpdate::StreamDelta("working".to_string()), &mut ctx);
        mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

        let envelopes = [
            serde_json::from_str::<RuntimeEnvelope>(&envelope_rx.recv().await.unwrap()).unwrap(),
            serde_json::from_str::<RuntimeEnvelope>(&envelope_rx.recv().await.unwrap()).unwrap(),
            serde_json::from_str::<RuntimeEnvelope>(&envelope_rx.recv().await.unwrap()).unwrap(),
            serde_json::from_str::<RuntimeEnvelope>(&envelope_rx.recv().await.unwrap()).unwrap(),
            serde_json::from_str::<RuntimeEnvelope>(&envelope_rx.recv().await.unwrap()).unwrap(),
        ];

        let seqs: Vec<u64> = envelopes.iter().map(|envelope| envelope.seq).collect();
        assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
        assert!(matches!(envelopes[0].event, RuntimeEvent::TurnStart { .. }));
        let final_text_index = match &envelopes[1].event {
            RuntimeEvent::TranscriptBlockStart {
                index,
                block: StreamBlock::FinalText { content },
            } => {
                assert!(content.is_empty());
                *index
            }
            other => panic!("expected transcript final-text block start, got {other:?}"),
        };
        assert!(matches!(
            envelopes[2].event,
            RuntimeEvent::TranscriptBlockDelta {
                index,
                ref delta,
            } if index == final_text_index && delta == "working"
        ));
        assert!(matches!(
            envelopes[3].event,
            RuntimeEvent::TranscriptBlockComplete { index } if index == final_text_index
        ));
        assert!(matches!(
            envelopes[4].event,
            RuntimeEvent::TurnEnd { ref status, .. } if status == "completed"
        ));
    }

    #[tokio::test]
    async fn test_local_api_mode_emits_transcript_block_events() {
        let (envelope_tx, mut envelope_rx) = mpsc::unbounded_channel();
        let quit = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(LocalApiTaskShared::new(
            "task-transcript".to_string(),
            envelope_tx,
            quit,
            crate::runtime::delta_accumulator::DEFAULT_DELTA_ACCUMULATOR_MEMORY_WATERMARK_BYTES,
        )));
        let mut mode = LocalApiMode::new(Arc::clone(&shared));
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation =
            ConversationManager::new(client, ToolOperator::new(std::env::temp_dir()));
        let (update_tx, _update_rx) = mpsc::unbounded_channel();
        let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());

        mode.on_user_input("review src/local_api.rs".to_string(), &mut ctx);
        let _: RuntimeEnvelope = serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();

        mode.on_model_update(
            UiUpdate::TranscriptLine("[edit loop: running validation]".to_string()),
            &mut ctx,
        );
        mode.on_model_update(
            UiUpdate::StreamBlockStart {
                index: 0,
                block: StreamBlock::ToolCall {
                    id: "provider-call-1".to_string(),
                    name: "read_file".to_string(),
                    input: json!({"path":"src/local_api.rs"}),
                    status: crate::state::ToolStatus::Pending,
                },
            },
            &mut ctx,
        );
        mode.on_model_update(
            UiUpdate::StreamBlockDelta {
                index: 0,
                delta: "{\"path\":\"src/local_api.rs\"}".to_string(),
            },
            &mut ctx,
        );
        mode.on_model_update(UiUpdate::StreamBlockComplete { index: 0 }, &mut ctx);

        let transcript_line: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        let transcript_block_start: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        let tool_call: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        let tool_call_arguments_delta: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        let transcript_block_delta: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        let transcript_block_complete: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();

        assert!(matches!(
            transcript_line.event,
            RuntimeEvent::TranscriptLine { ref line }
                if line == "[edit loop: running validation]"
        ));
        assert!(matches!(
            transcript_block_start.event,
            RuntimeEvent::TranscriptBlockStart {
                index: 0,
                block: StreamBlock::ToolCall {
                    ref name,
                    ref input,
                    status: crate::state::ToolStatus::Pending,
                    ..
                }
            } if name == "read_file" && input["path"] == "src/local_api.rs"
        ));
        assert!(matches!(
            tool_call.event,
            RuntimeEvent::ToolCallStarted {
                ref tool_name,
                ref arguments,
                ..
            } if tool_name == "read_file" && arguments["path"] == "src/local_api.rs"
        ));
        assert!(matches!(
            tool_call_arguments_delta.event,
            RuntimeEvent::ToolCallArgumentsDelta {
                ref tool_name,
                ref delta,
                ..
            } if tool_name.as_deref() == Some("read_file")
                && delta == "{\"path\":\"src/local_api.rs\"}"
        ));
        assert!(matches!(
            transcript_block_delta.event,
            RuntimeEvent::TranscriptBlockDelta {
                index: 0,
                ref delta,
            } if delta == "{\"path\":\"src/local_api.rs\"}"
        ));
        assert!(matches!(
            transcript_block_complete.event,
            RuntimeEvent::TranscriptBlockComplete { index: 0 }
        ));

        {
            let shared = shared.lock().unwrap();
            assert!(matches!(
                shared.recent_peer_events.front(),
                Some(PeerDeltaEvent::ToolStart { .. })
            ));
            assert!(
                shared
                    .recent_peer_events
                    .iter()
                    .any(|event| matches!(event, PeerDeltaEvent::ToolDelta { .. }))
            );
            assert!(matches!(
                shared.recent_peer_events.back(),
                Some(PeerDeltaEvent::ToolFinish { .. })
            ));
        }
    }

    #[tokio::test]
    async fn test_local_api_mode_waits_for_command_sessions_before_turn_end() {
        let (envelope_tx, mut envelope_rx) = mpsc::unbounded_channel();
        let quit = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(LocalApiTaskShared {
            normalizer: RuntimeEnvelopeNormalizer::new("task-command-session"),
            envelope_tx,
            peer_event_rx: mpsc::channel(1).1,
            recent_peer_events: VecDeque::new(),
            pending_approval: None,
            quit: Arc::clone(&quit),
            turn_in_progress: false,
            interrupted: false,
            active_command_sessions: BTreeSet::new(),
            turn_completion_pending: false,
        }));
        let mut mode = LocalApiMode::new(Arc::clone(&shared));
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation =
            ConversationManager::new(client, ToolOperator::new(std::env::temp_dir()));
        let (update_tx, _update_rx) = mpsc::unbounded_channel();
        let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());

        mode.on_user_input("run delayed command".to_string(), &mut ctx);
        let start: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        assert!(matches!(start.event, RuntimeEvent::TurnStart { .. }));

        mode.on_model_update(
            UiUpdate::CommandSessionStarted {
                session_id: 41,
                command: "echo delayed".to_string(),
            },
            &mut ctx,
        );
        mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

        {
            let shared = shared.lock().unwrap();
            assert!(shared.turn_in_progress);
            assert!(shared.turn_completion_pending);
            assert!(shared.active_command_sessions.contains(&41));
        }
        assert!(!quit.load(Ordering::SeqCst));
        assert!(envelope_rx.try_recv().is_err());

        mode.on_model_update(
            UiUpdate::CommandSessionFinished { session_id: 41 },
            &mut ctx,
        );

        let final_envelope: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        assert!(matches!(
            final_envelope.event,
            RuntimeEvent::TurnEnd { ref status, .. } if status == "completed"
        ));

        {
            let shared = shared.lock().unwrap();
            assert!(!shared.turn_in_progress);
            assert!(!shared.turn_completion_pending);
            assert!(shared.active_command_sessions.is_empty());
        }
        assert!(quit.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_local_api_mode_error_emits_failed_turn_end() {
        let (envelope_tx, mut envelope_rx) = mpsc::unbounded_channel();
        let quit = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(LocalApiTaskShared {
            normalizer: RuntimeEnvelopeNormalizer::new("task-error"),
            envelope_tx,
            peer_event_rx: mpsc::channel(1).1,
            recent_peer_events: VecDeque::new(),
            pending_approval: None,
            quit,
            turn_in_progress: false,
            interrupted: false,
            active_command_sessions: BTreeSet::new(),
            turn_completion_pending: false,
        }));
        let mut mode = LocalApiMode::new(Arc::clone(&shared));
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation =
            ConversationManager::new(client, ToolOperator::new(std::env::temp_dir()));
        let (update_tx, _update_rx) = mpsc::unbounded_channel();
        let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());

        mode.on_user_input("review src/local_api.rs".to_string(), &mut ctx);
        let start: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        assert!(matches!(start.event, RuntimeEvent::TurnStart { .. }));
        mode.on_model_update(UiUpdate::Error("stream failed".to_string()), &mut ctx);

        let error: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        let final_envelope: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();

        assert!(matches!(
            error.event,
            RuntimeEvent::Error {
                ref code,
                ref message,
                recoverable: false,
            } if code == "runtime_error" && message == "stream failed"
        ));
        assert!(matches!(
            final_envelope.event,
            RuntimeEvent::TurnEnd { ref status, .. } if status == "failed"
        ));
    }
}
