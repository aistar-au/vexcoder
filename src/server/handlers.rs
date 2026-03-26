use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

use super::sse::runtime_sse_response;
use super::util::{bad_request, conflict, internal_error, not_found};
use super::SSE_KEEPALIVE_INTERVAL;
use crate::agents::{load_agents_config, IsolationPolicy};
use crate::app::execute_facade_runtime;
use crate::local_api::{
    ActiveTask, FrontendCommand, LocalApiMode, LocalApiState, LocalApiTaskShared,
};
use crate::runtime::json_handoff::{RuntimeEnvelopeNormalizer, RuntimeRequest, TurnEndContext};
use crate::runtime::{SessionTask, TaskState, WorktreeLeaseManager};

use super::ControlResponse;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    ok: bool,
    service: &'static str,
    version: u16,
}

#[derive(Debug, Serialize)]
pub struct SchemaBundle {
    version: u16,
    request_schema: Value,
    envelope_schema: Value,
}

#[derive(Debug, Serialize)]
pub struct AgentsResponse {
    available: bool,
    agents: Vec<AgentDescriptor>,
    teams: Vec<TeamDescriptor>,
}

#[derive(Debug, Serialize)]
pub struct AgentDescriptor {
    name: String,
    profile: String,
    isolation: String,
    live_session_tasks: usize,
}

#[derive(Debug, Serialize)]
pub struct TeamDescriptor {
    name: String,
    members: Vec<String>,
    scheduler: String,
}

#[derive(Debug, Deserialize)]
pub struct DelegateRequest {
    parent_task_id: Option<String>,
    agent_id: String,
    prompt: String,
}

#[derive(Debug, Serialize)]
pub struct DelegateResponse {
    ok: bool,
    parent_task_id: String,
    session_task_id: String,
}

#[derive(Debug, Serialize)]
pub struct WatchSnapshot {
    kind: &'static str,
    id: String,
    parent_task_id: Option<String>,
    agent_id: Option<String>,
    status: String,
    worktree_path: Option<String>,
}

fn internal_anyhow(_: anyhow::Error) -> (StatusCode, Json<ControlResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ControlResponse {
            ok: false,
            reason: Some("internal_error"),
        }),
    )
}

pub async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        service: "vexcoder-local-api",
        version: 1,
    })
}

pub async fn schema_handler() -> Result<Json<SchemaBundle>, (StatusCode, Json<ControlResponse>)> {
    let request_schema: Value =
        serde_json::from_str(include_str!("../../schemas/runtime_request_v1.json"))
            .map_err(internal_error)?;
    let envelope_schema: Value =
        serde_json::from_str(include_str!("../../schemas/runtime_envelope_v1.json"))
            .map_err(internal_error)?;
    Ok(Json(SchemaBundle {
        version: 1,
        request_schema,
        envelope_schema,
    }))
}

pub async fn agents_handler(
    State(state): State<LocalApiState>,
) -> Result<Json<AgentsResponse>, (StatusCode, Json<ControlResponse>)> {
    let config = load_agents_config(&state.config.working_dir).map_err(internal_anyhow)?;
    let Some(config) = config else {
        return Ok(Json(AgentsResponse {
            available: false,
            agents: Vec::new(),
            teams: Vec::new(),
        }));
    };

    let mut live_counts = std::collections::HashMap::<String, usize>::new();
    for file in TaskState::state_files_from(&state.config.working_dir) {
        let task_state = TaskState::load(&file.dir, &file.id).map_err(internal_anyhow)?;
        for session_task in task_state.session_tasks {
            if session_task.lifecycle_state.is_live() {
                *live_counts.entry(session_task.agent_id).or_default() += 1;
            }
        }
    }

    Ok(Json(AgentsResponse {
        available: true,
        agents: config
            .agent_profiles
            .into_iter()
            .map(|agent| AgentDescriptor {
                live_session_tasks: live_counts.remove(&agent.name).unwrap_or_default(),
                name: agent.name,
                profile: agent.profile,
                isolation: match agent.isolation {
                    IsolationPolicy::Worktree => "worktree".to_string(),
                    IsolationPolicy::Shared => "shared".to_string(),
                },
            })
            .collect(),
        teams: config
            .team_definitions
            .into_iter()
            .map(|team| TeamDescriptor {
                name: team.name,
                members: team.members,
                scheduler: format!("{:?}", team.scheduler),
            })
            .collect(),
    }))
}

pub async fn delegate_handler(
    State(state): State<LocalApiState>,
    Json(request): Json<DelegateRequest>,
) -> Result<Json<DelegateResponse>, (StatusCode, Json<ControlResponse>)> {
    if request.agent_id.trim().is_empty() || request.prompt.trim().is_empty() {
        return Err(bad_request("invalid_delegate_request"));
    }

    let config = load_agents_config(&state.config.working_dir).map_err(internal_anyhow)?;
    let Some(config) = config else {
        return Err(bad_request("agents_config_missing"));
    };
    let Some(agent) = config
        .agent_profiles
        .iter()
        .find(|agent| agent.name == request.agent_id)
    else {
        return Err(not_found("agent_not_found"));
    };

    let parent_task_id = request.parent_task_id.unwrap_or_else(new_server_task_id);
    let state_dir = TaskState::state_dir_from(&state.config.working_dir);
    let mut parent_state = TaskState::load(&state_dir, &parent_task_id)
        .unwrap_or_else(|_| TaskState::new(parent_task_id.clone()));

    let mut session_task = SessionTask::new(
        parent_task_id.clone(),
        request.agent_id,
        request.prompt,
        None,
    );
    if agent.isolation == IsolationPolicy::Worktree {
        let lease_manager = WorktreeLeaseManager::new(&state_dir);
        let lease = lease_manager
            .lease_for_task(&session_task.id, Some(&parent_task_id))
            .map_err(internal_anyhow)?;
        session_task.worktree_path = Some(lease.path);
    }

    let session_task_id = session_task.id.clone();
    parent_state.add_session_task(session_task);
    parent_state.save(&state_dir).map_err(internal_anyhow)?;

    Ok(Json(DelegateResponse {
        ok: true,
        parent_task_id,
        session_task_id,
    }))
}

pub async fn watch_handler(
    State(state): State<LocalApiState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ControlResponse>)> {
    let snapshot = if let Ok(task_state) =
        TaskState::load_from_search_dirs_from(&state.config.working_dir, &id)
    {
        WatchSnapshot {
            kind: "task",
            id: task_state.id,
            parent_task_id: task_state.parent_task_id,
            agent_id: task_state.agent_id,
            status: format!("{:?}", task_state.status),
            worktree_path: task_state
                .worktree_path
                .as_ref()
                .map(|path| path.display().to_string()),
        }
    } else if let Some((parent_state, session_task)) =
        TaskState::find_session_task_in_saved_states(&state.config.working_dir, &id)
            .map_err(internal_anyhow)?
    {
        WatchSnapshot {
            kind: "session-task",
            id: session_task.id,
            parent_task_id: Some(parent_state.id),
            agent_id: Some(session_task.agent_id),
            status: format!("{:?}", session_task.lifecycle_state),
            worktree_path: session_task
                .worktree_path
                .as_ref()
                .map(|path| path.display().to_string()),
        }
    } else {
        return Err(not_found("task_not_found"));
    };

    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let payload = serde_json::to_string(&snapshot).map_err(internal_error)?;
    let _ = tx.send(payload);
    drop(tx);

    Ok((
        StatusCode::OK,
        [(axum::http::header::CACHE_CONTROL, "no-cache")],
        runtime_sse_response(rx, SSE_KEEPALIVE_INTERVAL),
    ))
}

pub async fn turns_handler(
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

    Ok((
        StatusCode::OK,
        [(axum::http::header::CACHE_CONTROL, "no-cache")],
        runtime_sse_response(envelope_rx, SSE_KEEPALIVE_INTERVAL),
    ))
}

pub async fn interrupt_handler(
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

pub async fn approve_handler(
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
    config: crate::config::Config,
    task_id: String,
    input: String,
    shared: Arc<Mutex<LocalApiTaskShared>>,
    interrupt_rx: mpsc::UnboundedReceiver<FrontendCommand>,
) -> anyhow::Result<()> {
    use crate::local_api::LocalApiFrontend;

    let mode = LocalApiMode::new(Arc::clone(&shared));
    let quit = shared
        .lock()
        .expect("local api shared lock poisoned")
        .quit
        .clone();
    let mut frontend = LocalApiFrontend::new(input, interrupt_rx, quit);
    if let Some(warning) = execute_facade_runtime(&config, mode, &mut frontend)
        .await?
        .notes_warning
    {
        eprintln!("{warning}");
    }
    if !frontend.should_quit() {
        return Err(anyhow::anyhow!(
            "local api runtime exited before signalling completion for {task_id}"
        ));
    }
    Ok(())
}

pub fn new_server_task_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("task-{millis}")
}
