use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::UnboundedReceiverStream;

use super::sse::runtime_sse_response;
use super::util::{bad_request, conflict, internal_error, not_found};
use super::{SSE_KEEPALIVE_INTERVAL, SSE_KEEPALIVE_TEXT};
use crate::app::{
    execute_facade_runtime, facade_delegate_session_task, facade_get_session_task,
    facade_list_agents, facade_list_session_tasks, facade_list_tasks, facade_poll_join,
    facade_release_session_task, facade_schedule_team, facade_update_session_task_status,
    facade_watch_snapshot, DelegateError, ScheduleTeamError, SessionTaskStatusError,
};
use crate::local_api::{
    ActiveTask, FrontendCommand, LocalApiMode, LocalApiState, LocalApiTaskShared,
};
use crate::runtime::json_handoff::{RuntimeEnvelopeNormalizer, RuntimeRequest, TurnEndContext};

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
    max_parallel_tasks: u32,
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

fn internal_anyhow(err: anyhow::Error) -> (StatusCode, Json<ControlResponse>) {
    tracing::error!(%err, "handler returned internal error");
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
    let listing = facade_list_agents(&state.config.working_dir).map_err(internal_anyhow)?;

    Ok(Json(AgentsResponse {
        available: listing.available,
        agents: listing
            .agents
            .into_iter()
            .map(|a| AgentDescriptor {
                name: a.name,
                profile: a.profile,
                isolation: a.isolation,
                max_parallel_tasks: a.max_parallel_tasks,
                live_session_tasks: a.live_session_tasks,
            })
            .collect(),
        teams: listing
            .teams
            .into_iter()
            .map(|t| TeamDescriptor {
                name: t.name,
                members: t.members,
                scheduler: t.scheduler,
            })
            .collect(),
    }))
}

#[tracing::instrument(skip_all)]
pub async fn delegate_handler(
    State(state): State<LocalApiState>,
    Json(request): Json<DelegateRequest>,
) -> Result<Json<DelegateResponse>, (StatusCode, Json<ControlResponse>)> {
    if request.agent_id.trim().is_empty() || request.prompt.trim().is_empty() {
        return Err(bad_request("invalid_delegate_request"));
    }

    let result = facade_delegate_session_task(
        &state.config.working_dir,
        request.parent_task_id,
        &request.agent_id,
        &request.prompt,
    )
    .map_err(|e| match e {
        DelegateError::AgentNotFound => not_found("agent_not_found"),
        DelegateError::AgentsConfigMissing => bad_request("agents_config_missing"),
        DelegateError::ParentTaskIdRequired => bad_request("parent_task_id_required"),
        DelegateError::ConcurrencyLimitReached => conflict("concurrency_limit_reached"),
        DelegateError::PromptTooLong => bad_request("prompt_too_long"),
        DelegateError::Internal(inner) => internal_anyhow(inner),
    })?;

    Ok(Json(DelegateResponse {
        ok: true,
        parent_task_id: result.parent_task_id,
        session_task_id: result.session_task_id,
    }))
}

#[tracing::instrument(skip_all, fields(id = %id))]
pub async fn watch_handler(
    State(state): State<LocalApiState>,
    Path(id): Path<String>,
) -> Result<Json<WatchSnapshot>, (StatusCode, Json<ControlResponse>)> {
    let snapshot = facade_watch_snapshot(&state.config.working_dir, &id)
        .map_err(internal_anyhow)?
        .ok_or_else(|| not_found("task_not_found"))?;

    Ok(Json(WatchSnapshot {
        kind: snapshot.kind,
        id: snapshot.id,
        parent_task_id: snapshot.parent_task_id,
        agent_id: snapshot.agent_id,
        status: snapshot.status,
        worktree_path: snapshot.worktree_path,
    }))
}

/// Release a session task: transition it to `Completed` and drop the worktree
/// lease.  The caller is responsible for ensuring that no agent process is
/// still running in the worktree before calling this endpoint.
///
/// Returns 200 `{ ok: true }` on success, 404 when the session task is not
/// found in any saved task-state file.
#[tracing::instrument(skip_all, fields(id = %id))]
pub async fn release_session_task_handler(
    State(state): State<LocalApiState>,
    Path(id): Path<String>,
) -> Result<Json<ControlResponse>, (StatusCode, Json<ControlResponse>)> {
    let released =
        facade_release_session_task(&state.config.working_dir, &id).map_err(internal_anyhow)?;
    if released {
        if let Some(snapshot) =
            facade_get_session_task(&state.config.working_dir, &id).map_err(internal_anyhow)?
        {
            state.publish_session_task_snapshot(snapshot);
        }
        Ok(Json(ControlResponse {
            ok: true,
            reason: None,
        }))
    } else {
        Err(not_found("session_task_not_found"))
    }
}

// ---------------------------------------------------------------------------
// ADR-034 Phase C: subtask orchestration handlers
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ScheduleTeamRequest {
    pub parent_task_id: String,
    pub prompt: String,
}

#[derive(Debug, Serialize)]
pub struct ScheduleTeamResponse {
    pub ok: bool,
    pub parent_task_id: String,
    pub session_task_ids: Vec<String>,
    pub scheduler: String,
}

#[derive(Debug, Serialize)]
pub struct JoinStatusResponse {
    pub pending: bool,
    pub all_done: bool,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub summaries: Vec<JoinSummaryEntry>,
}

#[derive(Debug, Serialize)]
pub struct JoinSummaryEntry {
    pub agent_id: String,
    pub summary: String,
}

/// Decompose a parent task into session tasks for a named team.
///
/// `POST /v1/teams/{team_name}/schedule` with body `{ parent_task_id, prompt }`
#[tracing::instrument(skip_all, fields(team_name = %team_name))]
pub async fn schedule_team_handler(
    State(state): State<LocalApiState>,
    Path(team_name): Path<String>,
    Json(request): Json<ScheduleTeamRequest>,
) -> Result<Json<ScheduleTeamResponse>, (StatusCode, Json<ControlResponse>)> {
    let result = facade_schedule_team(
        &state.config.working_dir,
        &request.parent_task_id,
        &team_name,
        &request.prompt,
    )
    .map_err(|e| match e {
        ScheduleTeamError::AgentsConfigMissing => bad_request("agents_config_missing"),
        ScheduleTeamError::TeamNotFound => not_found("team_not_found"),
        ScheduleTeamError::ParentTaskIdRequired => bad_request("parent_task_id_required"),
        ScheduleTeamError::PromptRequired => bad_request("prompt_required"),
        ScheduleTeamError::ConcurrencyLimitReached => conflict("concurrency_limit_reached"),
        ScheduleTeamError::PromptTooLong => bad_request("prompt_too_long"),
        ScheduleTeamError::Internal(inner) => internal_anyhow(inner),
    })?;

    Ok(Json(ScheduleTeamResponse {
        ok: true,
        parent_task_id: result.parent_task_id,
        session_task_ids: result.session_task_ids,
        scheduler: result.scheduler,
    }))
}

/// Check the fan-out join gate for a parent task.
///
/// `GET /v1/tasks/{task_id}/join-status`
#[tracing::instrument(skip_all, fields(task_id = %task_id))]
pub async fn join_status_handler(
    State(state): State<LocalApiState>,
    Path(task_id): Path<String>,
) -> Result<Json<JoinStatusResponse>, (StatusCode, Json<ControlResponse>)> {
    let outcome = facade_poll_join(&state.config.working_dir, &task_id).map_err(internal_anyhow)?;

    match outcome {
        None => Ok(Json(JoinStatusResponse {
            pending: true,
            all_done: false,
            completed: 0,
            failed: 0,
            cancelled: 0,
            summaries: vec![],
        })),
        Some(o) => Ok(Json(JoinStatusResponse {
            pending: false,
            all_done: o.all_done,
            completed: o.completed,
            failed: o.failed,
            cancelled: o.cancelled,
            summaries: o
                .summaries
                .into_iter()
                .map(|(agent_id, summary)| JoinSummaryEntry { agent_id, summary })
                .collect(),
        })),
    }
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

    let mut shared = task.shared.lock().unwrap_or_else(|e| e.into_inner());
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
            let mut shared = shared.lock().unwrap_or_else(|e| e.into_inner());
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
        .unwrap_or_else(|e| e.into_inner())
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

// ---------------------------------------------------------------------------
// Phase E — LocalApi session-task projection response types and handlers
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct TaskSummaryResponse {
    pub id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub session_task_count: usize,
    pub live_session_task_count: usize,
}

#[derive(Debug, Serialize)]
pub struct SessionTaskSnapshotResponse {
    pub id: String,
    pub parent_task_id: String,
    pub agent_id: String,
    pub lifecycle_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    pub updated_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handoff_summary: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSessionTaskStatusRequest {
    pub status: String,
}

/// `GET /v1/tasks`
#[tracing::instrument(skip_all)]
pub async fn list_tasks_handler(
    State(state): State<LocalApiState>,
) -> Result<Json<Vec<TaskSummaryResponse>>, (StatusCode, Json<ControlResponse>)> {
    let summaries = facade_list_tasks(&state.config.working_dir).map_err(internal_anyhow)?;
    Ok(Json(
        summaries
            .into_iter()
            .map(|s| TaskSummaryResponse {
                id: s.id,
                status: s.status,
                parent_task_id: s.parent_task_id,
                agent_id: s.agent_id,
                session_task_count: s.session_task_count,
                live_session_task_count: s.live_session_task_count,
            })
            .collect(),
    ))
}

/// `GET /v1/session-tasks`
#[tracing::instrument(skip_all)]
pub async fn list_session_tasks_handler(
    State(state): State<LocalApiState>,
) -> Result<Json<Vec<SessionTaskSnapshotResponse>>, (StatusCode, Json<ControlResponse>)> {
    let tasks = facade_list_session_tasks(&state.config.working_dir).map_err(internal_anyhow)?;
    Ok(Json(tasks.into_iter().map(snapshot_to_response).collect()))
}

/// `GET /v1/session-tasks/{id}`
#[tracing::instrument(skip_all, fields(id = %id))]
pub async fn get_session_task_handler(
    State(state): State<LocalApiState>,
    Path(id): Path<String>,
) -> Result<Json<SessionTaskSnapshotResponse>, (StatusCode, Json<ControlResponse>)> {
    let snap = facade_get_session_task(&state.config.working_dir, &id).map_err(internal_anyhow)?;
    match snap {
        Some(s) => Ok(Json(snapshot_to_response(s))),
        None => Err(not_found("session_task_not_found")),
    }
}

/// `PATCH /v1/session-tasks/{id}/status`
#[tracing::instrument(skip_all, fields(id = %id))]
pub async fn update_session_task_status_handler(
    State(state): State<LocalApiState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateSessionTaskStatusRequest>,
) -> Result<Json<SessionTaskSnapshotResponse>, (StatusCode, Json<ControlResponse>)> {
    match facade_update_session_task_status(&state.config.working_dir, &id, &body.status) {
        Ok(snap) => {
            state.publish_session_task_snapshot(snap.clone());
            Ok(Json(snapshot_to_response(snap)))
        }
        Err(SessionTaskStatusError::NotFound) => Err(not_found("session_task_not_found")),
        Err(SessionTaskStatusError::InvalidStatus) => Err(bad_request("invalid_status")),
        Err(SessionTaskStatusError::TransitionNotAllowed) => {
            Err(conflict("transition_not_allowed"))
        }
        Err(SessionTaskStatusError::Internal(err)) => Err(internal_anyhow(err)),
    }
}

fn snapshot_to_response(s: crate::app::FacadeSessionTaskSnapshot) -> SessionTaskSnapshotResponse {
    SessionTaskSnapshotResponse {
        id: s.id,
        parent_task_id: s.parent_task_id,
        agent_id: s.agent_id,
        lifecycle_state: s.lifecycle_state,
        worktree_path: s.worktree_path,
        started_at_ms: s.started_at_ms,
        updated_at_ms: s.updated_at_ms,
        handoff_summary: s.handoff_summary,
    }
}

fn session_task_event(
    snapshot: crate::app::FacadeSessionTaskSnapshot,
) -> Result<Event, serde_json::Error> {
    serde_json::to_string(&snapshot_to_response(snapshot))
        .map(|data| Event::default().event("session_task").data(data))
}

fn lifecycle_state_is_terminal(state: &str) -> bool {
    matches!(state, "failed" | "cancelled" | "completed")
}

// ---------------------------------------------------------------------------
// ADR-034 Phase E2 — watch-stream projection
// ---------------------------------------------------------------------------

/// `GET /v1/session-tasks/{id}/watch`
///
/// Returns an SSE stream.  The server emits a `session_task` event each time
/// the session task's `updated_at_ms` timestamp advances.  The initial
/// snapshot is always emitted on connect.  Live updates are fanned out through
/// `LocalApiState`'s in-process broadcast channel while the persisted
/// task-state files remain the durable source of truth for reconnects.  The
/// stream terminates automatically once the session task reaches a terminal
/// state (`failed`, `cancelled`, or `completed`).
///
/// Returns 404 when no session task with the given id exists at connection
/// time.
#[tracing::instrument(skip_all, fields(id = %id))]
pub async fn watch_session_task_handler(
    State(state): State<LocalApiState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ControlResponse>)> {
    let mut updates = state.subscribe_session_task_events();

    let initial_snapshot = facade_get_session_task(&state.config.working_dir, &id)
        .map_err(internal_anyhow)?
        .ok_or_else(|| not_found("session_task_not_found"))?;

    let (tx, rx) = mpsc::unbounded_channel::<Result<Event, Infallible>>();
    let initial_is_terminal = lifecycle_state_is_terminal(&initial_snapshot.lifecycle_state);
    let mut last_updated_at = initial_snapshot.updated_at_ms;

    let initial_event = session_task_event(initial_snapshot).map_err(|err| {
        internal_anyhow(anyhow::anyhow!(
            "failed to serialize session-task watch snapshot: {err}"
        ))
    })?;
    tx.send(Ok(initial_event)).map_err(|_| {
        internal_anyhow(anyhow::anyhow!("failed to seed session-task watch stream"))
    })?;

    if !initial_is_terminal {
        tokio::spawn(async move {
            loop {
                let snapshot = match updates.recv().await {
                    Ok(snapshot) if snapshot.id == id => snapshot,
                    Ok(_) => continue,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                };

                let is_terminal = lifecycle_state_is_terminal(&snapshot.lifecycle_state);
                if snapshot.updated_at_ms <= last_updated_at {
                    if is_terminal {
                        break;
                    }
                    continue;
                }
                last_updated_at = snapshot.updated_at_ms;

                let event = match session_task_event(snapshot) {
                    Ok(event) => event,
                    Err(_) => break,
                };
                if tx.send(Ok(event)).is_err() {
                    break;
                }
                if is_terminal {
                    break;
                }
            }
        });
    }

    let stream = UnboundedReceiverStream::new(rx);
    Ok((
        StatusCode::OK,
        [(axum::http::header::CACHE_CONTROL, "no-cache")],
        Sse::new(stream).keep_alive(
            KeepAlive::new()
                .interval(SSE_KEEPALIVE_INTERVAL)
                .text(SSE_KEEPALIVE_TEXT),
        ),
    ))
}
