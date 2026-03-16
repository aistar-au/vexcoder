use crate::api::ApiClient;
use crate::config::Config;
use crate::runtime::context::RuntimeContext;
use crate::runtime::frontend::{FrontendAdapter, UserInputEvent};
use crate::runtime::json_handoff::{
    runtime_approval_request_event, RuntimeEnvelope, RuntimeEnvelopeNormalizer, RuntimeEvent,
    RuntimeRequest, TurnEndContext,
};
use crate::runtime::mode::RuntimeMode;
use crate::runtime::project_instructions::{load_project_instructions, LoadResult};
use crate::runtime::r#loop::Runtime;
use crate::runtime::UiUpdate;
use crate::session_notes::build_api_client_with_notes;
use crate::state::{ConversationManager, TurnToolPolicy};
use crate::tools::ToolOperator;
use anyhow::{anyhow, Context, Result};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::sync::Mutex as AsyncMutex;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

pub const DEFAULT_LOCAL_API_HOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
pub const DEFAULT_LOCAL_API_PORT: u16 = 6274;

#[derive(Clone)]
pub struct LocalApiState {
    config: Config,
    tasks: Arc<AsyncMutex<HashMap<String, ActiveTask>>>,
}

struct ActiveTask {
    interrupt_tx: mpsc::UnboundedSender<FrontendCommand>,
    shared: Arc<Mutex<LocalApiTaskShared>>,
}

struct LocalApiTaskShared {
    normalizer: RuntimeEnvelopeNormalizer,
    envelope_tx: mpsc::UnboundedSender<String>,
    pending_approval: Option<PendingApproval>,
    quit: Arc<AtomicBool>,
    turn_in_progress: bool,
    interrupted: bool,
}

struct PendingApproval {
    capability: String,
    scope: String,
    response_tx: tokio::sync::oneshot::Sender<bool>,
}

enum FrontendCommand {
    Interrupt,
}

struct LocalApiFrontend {
    initial_input: Option<String>,
    command_rx: mpsc::UnboundedReceiver<FrontendCommand>,
    quit: Arc<AtomicBool>,
}

impl LocalApiFrontend {
    fn new(
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

struct LocalApiMode {
    shared: Arc<Mutex<LocalApiTaskShared>>,
}

impl LocalApiMode {
    fn new(shared: Arc<Mutex<LocalApiTaskShared>>) -> Self {
        Self { shared }
    }

    fn emit_envelopes(shared: &mut LocalApiTaskShared, envelopes: Vec<RuntimeEnvelope>) {
        for envelope in envelopes {
            if let Ok(json) = serde_json::to_string(&envelope) {
                let _ = shared.envelope_tx.send(json);
            }
        }
    }
}

impl RuntimeMode for LocalApiMode {
    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext) {
        let mut shared = self.shared.lock().expect("local api shared lock poisoned");
        shared.turn_in_progress = true;
        shared.interrupted = false;
        let start = shared.normalizer.start_turn(1, Some(input.clone()));
        Self::emit_envelopes(&mut shared, vec![start]);
        drop(shared);

        ctx.start_turn_with_system_prompt_and_policy(input, None, TurnToolPolicy::Default);
    }

    fn on_interrupt(&mut self, ctx: &mut RuntimeContext) {
        let mut shared = self.shared.lock().expect("local api shared lock poisoned");
        if !shared.turn_in_progress {
            return;
        }
        shared.interrupted = true;
        drop(shared);
        ctx.cancel_turn();
    }

    fn on_model_update(&mut self, update: UiUpdate, _ctx: &mut RuntimeContext) {
        let mut shared = self.shared.lock().expect("local api shared lock poisoned");
        match update {
            UiUpdate::TranscriptLine(_) => {}
            UiUpdate::StreamDelta(text) => {
                let envelopes = shared
                    .normalizer
                    .normalize_ui_update(&UiUpdate::StreamDelta(text), None);
                Self::emit_envelopes(&mut shared, envelopes);
            }
            UiUpdate::StreamBlockStart { block, .. } => {
                let envelopes = shared.normalizer.normalize_stream_block(&block);
                Self::emit_envelopes(&mut shared, envelopes);
            }
            UiUpdate::StreamBlockDelta { .. } | UiUpdate::StreamBlockComplete { .. } => {}
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
            UiUpdate::TurnComplete => {
                let envelopes = if shared.interrupted {
                    shared.normalizer.emit_cancelled(TurnEndContext::default())
                } else {
                    shared.normalizer.normalize_ui_update(
                        &UiUpdate::TurnComplete,
                        Some(TurnEndContext::default()),
                    )
                };
                shared.turn_in_progress = false;
                shared.quit.store(true, Ordering::SeqCst);
                Self::emit_envelopes(&mut shared, envelopes);
            }
            UiUpdate::Error(message) => {
                let envelopes = shared.normalizer.emit_error(
                    "runtime_error".to_string(),
                    message,
                    false,
                    TurnEndContext::default(),
                );
                shared.turn_in_progress = false;
                shared.quit.store(true, Ordering::SeqCst);
                Self::emit_envelopes(&mut shared, envelopes);
            }
            UiUpdate::CommandSessionStarted { .. }
            | UiUpdate::CommandSessionAttached { .. }
            | UiUpdate::EditLoopComplete { .. }
            | UiUpdate::CommandSessionFinished { .. } => {}
        }
    }

    fn is_turn_in_progress(&self) -> bool {
        self.shared
            .lock()
            .expect("local api shared lock poisoned")
            .turn_in_progress
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    ok: bool,
    service: &'static str,
    version: u16,
}

#[derive(Debug, Serialize)]
struct ControlResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct SchemaBundle {
    version: u16,
    request_schema: Value,
    envelope_schema: Value,
}

impl LocalApiState {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            tasks: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }
}

pub fn build_router(config: Config) -> Router {
    let state = LocalApiState::new(config);
    Router::new()
        .route("/v1/health", get(health_handler))
        .route("/v1/schema", get(schema_handler))
        .route("/v1/turns", post(turns_handler))
        .route("/v1/interrupt", post(interrupt_handler))
        .route("/v1/approve", post(approve_handler))
        .with_state(state)
}

pub async fn serve_local_api(config: Config, host: IpAddr, port: u16) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::new(host, port))
        .await
        .with_context(|| format!("failed to bind LocalApiServer on {host}:{port}"))?;
    axum::serve(listener, build_router(config))
        .await
        .context("LocalApiServer exited with an error")
}

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        service: "vexcoder-local-api",
        version: 1,
    })
}

async fn schema_handler() -> Result<Json<SchemaBundle>, (StatusCode, Json<ControlResponse>)> {
    let request_schema: Value =
        serde_json::from_str(include_str!("../schemas/runtime_request_v1.json"))
            .map_err(internal_error)?;
    let envelope_schema: Value =
        serde_json::from_str(include_str!("../schemas/runtime_envelope_v1.json"))
            .map_err(internal_error)?;
    Ok(Json(SchemaBundle {
        version: 1,
        request_schema,
        envelope_schema,
    }))
}

async fn turns_handler(
    State(state): State<LocalApiState>,
    Json(request): Json<RuntimeRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ControlResponse>)> {
    let RuntimeRequest::SubmitInput { task_id, input } = request else {
        return Err(bad_request("invalid_request_type"));
    };

    let task_id = task_id.unwrap_or_else(new_server_task_id);
    let (envelope_tx, envelope_rx) = mpsc::unbounded_channel::<String>();
    let (interrupt_tx, interrupt_rx) = mpsc::unbounded_channel::<FrontendCommand>();
    let quit = Arc::new(AtomicBool::new(false));
    let shared = Arc::new(Mutex::new(LocalApiTaskShared {
        normalizer: RuntimeEnvelopeNormalizer::new(task_id.clone()),
        envelope_tx,
        pending_approval: None,
        quit: Arc::clone(&quit),
        turn_in_progress: false,
        interrupted: false,
    }));

    {
        let mut tasks = state.tasks.lock().await;
        if tasks.contains_key(&task_id) {
            return Err(conflict("task_already_active"));
        }
        tasks.insert(
            task_id.clone(),
            ActiveTask {
                interrupt_tx,
                shared: Arc::clone(&shared),
            },
        );
    }

    spawn_local_api_task(state.clone(), task_id, input, shared, interrupt_rx);

    let stream = UnboundedReceiverStream::new(envelope_rx)
        .map(|payload| Ok::<Event, Infallible>(Event::default().event("runtime").data(payload)));

    Ok((
        StatusCode::OK,
        [(axum::http::header::CACHE_CONTROL, "no-cache")],
        Sse::new(stream),
    ))
}

async fn interrupt_handler(
    State(state): State<LocalApiState>,
    Json(request): Json<RuntimeRequest>,
) -> Result<Json<ControlResponse>, (StatusCode, Json<ControlResponse>)> {
    let RuntimeRequest::Interrupt { task_id } = request else {
        return Err(bad_request("invalid_request_type"));
    };

    let tasks = state.tasks.lock().await;
    let Some(task) = tasks.get(&task_id) else {
        return Err(not_found("task_not_found"));
    };
    task.interrupt_tx
        .send(FrontendCommand::Interrupt)
        .map_err(|_| not_found("task_not_found"))?;

    Ok(Json(ControlResponse {
        ok: true,
        reason: None,
    }))
}

async fn approve_handler(
    State(state): State<LocalApiState>,
    Json(request): Json<RuntimeRequest>,
) -> Result<Json<ControlResponse>, (StatusCode, Json<ControlResponse>)> {
    let task_id = match &request {
        RuntimeRequest::ApproveCapability { task_id, .. }
        | RuntimeRequest::DenyCapability { task_id, .. } => task_id.clone(),
        _ => return Err(bad_request("invalid_request_type")),
    };

    let tasks = state.tasks.lock().await;
    let Some(task) = tasks.get(&task_id) else {
        return Err(not_found("task_not_found"));
    };

    let mut shared = task.shared.lock().expect("local api shared lock poisoned");
    let Some(pending) = shared.pending_approval.take() else {
        return Err(conflict("no_pending_approval"));
    };

    let capability = match &request {
        RuntimeRequest::ApproveCapability { capability, .. }
        | RuntimeRequest::DenyCapability { capability, .. } => capability,
        _ => unreachable!(),
    };
    if pending.capability != *capability {
        shared.pending_approval = Some(pending);
        return Err(conflict("no_pending_approval"));
    }

    if let RuntimeRequest::ApproveCapability { scope, .. } = &request {
        if pending.scope != *scope {
            shared.pending_approval = Some(pending);
            return Err(conflict("no_pending_approval"));
        }
    }

    let approved = matches!(request, RuntimeRequest::ApproveCapability { .. });
    let _ = pending.response_tx.send(approved);
    let envelopes = shared.normalizer.normalize_runtime_request(&request);
    LocalApiMode::emit_envelopes(&mut shared, envelopes);

    Ok(Json(ControlResponse {
        ok: true,
        reason: None,
    }))
}

fn spawn_local_api_task(
    state: LocalApiState,
    task_id: String,
    input: String,
    shared: Arc<Mutex<LocalApiTaskShared>>,
    interrupt_rx: mpsc::UnboundedReceiver<FrontendCommand>,
) {
    tokio::spawn(async move {
        let result = run_local_api_task(
            state.config.clone(),
            task_id.clone(),
            input,
            Arc::clone(&shared),
            interrupt_rx,
        )
        .await;
        if let Err(error) = result {
            let mut shared = shared.lock().expect("local api shared lock poisoned");
            let envelopes = shared.normalizer.emit_error(
                "local_api_server".to_string(),
                error.to_string(),
                false,
                TurnEndContext::default(),
            );
            shared.quit.store(true, Ordering::SeqCst);
            LocalApiMode::emit_envelopes(&mut shared, envelopes);
        }
        state.tasks.lock().await.remove(&task_id);
    });
}

async fn run_local_api_task(
    config: Config,
    task_id: String,
    input: String,
    shared: Arc<Mutex<LocalApiTaskShared>>,
    interrupt_rx: mpsc::UnboundedReceiver<FrontendCommand>,
) -> Result<()> {
    let client = build_local_api_client(&config)?;
    let operator = ToolOperator::new(config.working_dir.clone());
    let conversation = ConversationManager::new_with_hooks(client, operator, config.hooks.clone());
    let (update_tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
    let mode = LocalApiMode::new(Arc::clone(&shared));
    let quit = shared
        .lock()
        .expect("local api shared lock poisoned")
        .quit
        .clone();
    let mut frontend = LocalApiFrontend::new(input, interrupt_rx, quit);
    let mut runtime = Runtime::new(mode, update_rx);
    runtime.run(&mut frontend, &mut ctx).await;
    if !frontend.should_quit() {
        return Err(anyhow!(
            "local api runtime exited before signalling completion for {task_id}"
        ));
    }
    Ok(())
}

fn build_local_api_client(config: &Config) -> Result<ApiClient> {
    let instructions = match load_project_instructions(
        &config.working_dir,
        config.max_project_instructions_tokens,
    ) {
        LoadResult::Loaded(project_instructions) => Some(project_instructions.content),
        LoadResult::OverBudget {
            path,
            estimated_tokens,
        } => {
            eprintln!(
                "[project instructions] {} skipped: estimated {} tokens exceeds budget of {}",
                path.display(),
                estimated_tokens,
                config.max_project_instructions_tokens,
            );
            None
        }
        LoadResult::NotFound => None,
    };

    let (client, notes_warning) = build_api_client_with_notes(config)?;
    if let Some(warning) = notes_warning {
        eprintln!("{warning}");
    }
    Ok(client.with_project_instructions(instructions))
}

fn new_server_task_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("task-{millis}")
}

fn bad_request(reason: &'static str) -> (StatusCode, Json<ControlResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ControlResponse {
            ok: false,
            reason: Some(reason),
        }),
    )
}

fn not_found(reason: &'static str) -> (StatusCode, Json<ControlResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(ControlResponse {
            ok: false,
            reason: Some(reason),
        }),
    )
}

fn conflict(reason: &'static str) -> (StatusCode, Json<ControlResponse>) {
    (
        StatusCode::CONFLICT,
        Json(ControlResponse {
            ok: false,
            reason: Some(reason),
        }),
    )
}

fn internal_error(_: serde_json::Error) -> (StatusCode, Json<ControlResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ControlResponse {
            ok: false,
            reason: Some("internal_error"),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::mock_client::MockApiClient;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_endpoint_returns_ok() {
        let router = build_router(Config::default_for_tui());
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_schema_endpoint_returns_bundle() {
        let router = build_router(Config::default_for_tui());
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/schema")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_local_api_mode_interrupt_emits_cancelled_turn_end() {
        let (envelope_tx, mut envelope_rx) = mpsc::unbounded_channel();
        let quit = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(LocalApiTaskShared {
            normalizer: RuntimeEnvelopeNormalizer::new("task-1"),
            envelope_tx,
            pending_approval: None,
            quit,
            turn_in_progress: false,
            interrupted: false,
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

        let assistant: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        let terminal: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        assert!(matches!(
            assistant.event,
            RuntimeEvent::AssistantMessage { .. }
        ));
        assert!(matches!(
            terminal.event,
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
            pending_approval: None,
            quit,
            turn_in_progress: false,
            interrupted: false,
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
}
