use super::*;
use crate::config::CompactionConfig;
use crate::config::UndoConfig;
use crate::runtime::frontend::{FrontendAdapter, InputOccurrence};
use crate::test_support::spawn_axum_server;
use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::Mutex;

type RouteLog = Arc<Mutex<Vec<String>>>;

struct HeadlessFrontend {
    inputs: VecDeque<String>,
    render_count: usize,
    ready_to_quit: bool,
}

impl HeadlessFrontend {
    fn new(inputs: Vec<&str>) -> Self {
        Self {
            inputs: inputs.into_iter().map(|value| value.to_string()).collect(),
            render_count: 0,
            ready_to_quit: false,
        }
    }
}

impl FrontendAdapter<TuiMode> for HeadlessFrontend {
    fn poll_user_input(&mut self, _mode: &TuiMode) -> Option<InputOccurrence> {
        self.inputs.pop_front().map(InputOccurrence::Text)
    }

    fn render(&mut self, _mode: &TuiMode) {
        self.render_count += 1;
        self.ready_to_quit = self.inputs.is_empty() && !_mode.is_pulse_in_progress();
    }

    fn should_quit(&self) -> bool {
        self.ready_to_quit
    }
}

fn make_config(temp: &std::path::Path) -> Config {
    Config {
        model_token: None,
        model_name: "mock-model".to_string(),
        model_url: "http://localhost:8000/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: temp.to_path_buf(),
        model_backend: crate::runtime::ModelBackendKind::LocalRuntime,
        model_protocol: crate::runtime::ModelProtocol::MessagesV1,
        tool_call_mode: crate::runtime::ToolCallMode::Structured,
        tool_policy: crate::runtime::ToolPolicy::Full,
        model_profile: ModelProfile::default_for_backend(
            crate::runtime::ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: UndoConfig::default(),
        search: crate::config::SearchConfig {
            auto_index: false,
            ..Default::default()
        },
        notes_path: None,
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
        api_client: crate::config::ApiClientConfig::default(),
        force: false,
        bypass_policy: false,
        expand_context: false,
    }
}

#[test]
fn build_runtime_with_resume_restores_task_and_grants() {
    let temp = tempfile::tempdir().unwrap();
    let mut state = TaskState::new("task-startup-resume".to_string());
    state
        .active_grants
        .insert(Capability::Network, ApprovalScope::Session);
    state.status = crate::runtime::TaskStatus::Running;

    let (runtime, _ctx) = build_runtime_with_resume(make_config(temp.path()), state).unwrap();
    assert_eq!(runtime.mode.task_doc.info.id, "task-startup-resume");
    assert_eq!(
        runtime
            .mode
            .task_doc
            .info
            .active_grants
            .get(&Capability::Network),
        Some(&ApprovalScope::Session)
    );
}

#[tokio::test]
async fn run_tui_session_populates_local_server_info_before_first_pulse() {
    async fn messages_get(State(log): State<RouteLog>) -> impl IntoResponse {
        log.lock().unwrap().push("GET /v1/messages".to_string());
        StatusCode::NOT_FOUND
    }

    async fn messages_post(
        State(log): State<RouteLog>,
        Json(payload): Json<Value>,
    ) -> impl IntoResponse {
        let stream = payload
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        log.lock()
            .unwrap()
            .push(format!("POST /v1/messages stream={stream}"));
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "messages endpoint should not be used" })),
        )
    }

    async fn chat_get(State(log): State<RouteLog>) -> impl IntoResponse {
        log.lock()
            .unwrap()
            .push("GET /v1/chat/completions".to_string());
        ([(header::CONTENT_TYPE, "text/event-stream")], "")
    }

    async fn chat_post(
        State(log): State<RouteLog>,
        Json(payload): Json<Value>,
    ) -> impl IntoResponse {
        let stream = payload
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        log.lock()
            .unwrap()
            .push(format!("POST /v1/chat/completions stream={stream}"));
        Json(json!({
            "id": "chatcmpl-runtime",
            "object": "chat.completion",
            "created": 1,
            "model": "local/test-model",
            "choices": [
                {
                    "index": 0,
                    "message": { "role": "assistant", "content": "OK" },
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 1,
                "total_tokens": 13
            }
        }))
    }

    let temp = tempfile::tempdir().unwrap();
    let route_log: RouteLog = Arc::new(Mutex::new(Vec::new()));
    let (server, addr) = spawn_axum_server(
        Router::new()
            .route("/v1/messages", get(messages_get).post(messages_post))
            .route("/v1/chat/completions", get(chat_get).post(chat_post))
            .with_state(route_log.clone()),
    )
    .await;

    let mut config = make_config(temp.path());
    config.model_url = format!("http://{addr}/v1");
    config.model_protocol = crate::runtime::ModelProtocol::MessagesV1;

    let mut frontend = HeadlessFrontend::new(vec!["hello"]);
    run_tui_session(config, None, &mut frontend).await.unwrap();
    server.abort();

    let events = route_log.lock().unwrap().clone();
    assert!(
        events
            .iter()
            .any(|event| event == "GET /v1/chat/completions"),
        "local runtime discovery must probe chat-compat endpoint; events: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event.starts_with("POST /v1/chat/completions")),
        "first pulse must route through discovered chat-compat endpoint; events: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| event.starts_with("POST /v1/messages")),
        "messages-v1 endpoint should not be used after preload; events: {events:?}"
    );
}

#[test]
fn compact_resets_turn_evidence_and_preserves_task_id() {
    let _temp = tempfile::tempdir().unwrap();
    let mut mode = TuiMode::new();
    let original_id = mode.current_task_id();
    let mut ctx = setup_ctx();
    mode.push_history_line("turn1".to_string());
    mode.on_user_input("/compact".to_string(), &mut ctx);
    assert_eq!(
        mode.current_task_id(),
        original_id,
        "compact must not change task-id"
    );
    assert!(
        mode.active_edit_loop.is_none(),
        "compact must clear edit loop"
    );
}
