use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::SSE_KEEPALIVE_INTERVAL;
use super::sse::runtime_sse_response;
use super::util::{ProblemDetailsResponse, bad_request, conflict, internal_error, not_found};
use crate::app::runtime_tokio::{spawn, sync::mpsc};
use crate::app::{
    DelegateError, ScheduleTeamError, execute_facade_runtime, facade_delegate_session_task,
    facade_get_session_task, facade_list_agents, facade_poll_join, facade_release_session_task,
    facade_schedule_team, facade_watch_rollup,
};
use crate::http_facade::{HeaderName, HeaderValue, StatusCode, header};
use crate::local_api::{
    ActiveTask, FrontendCommand, LocalApiMode, LocalApiState, LocalApiTaskShared,
};
use crate::privacy::{PrivacyStatement, privacy_statement};
use crate::runtime::json_handoff::{RuntimeRequest, TurnEndContext};
use crate::server::{
    SSE_CACHE_CONTROL_HEADER, SSE_PROXY_BUFFERING_DISABLED, SSE_PROXY_BUFFERING_HEADER,
};

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
pub struct WatchRollup {
    kind: &'static str,
    id: String,
    parent_task_id: Option<String>,
    agent_id: Option<String>,
    status: String,
    worktree_path: Option<String>,
}

fn internal_anyhow(err: anyhow::Error) -> ProblemDetailsResponse {
    tracing::error!(%err, "handler returned internal error");
    ProblemDetailsResponse::from_reason(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
}

pub async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        service: "vexcoder-local-api",
        version: 1,
    })
}

pub async fn schema_handler() -> Result<Json<SchemaBundle>, ProblemDetailsResponse> {
    let request_schema: Value =
        serde_json::from_str(include_str!("../../../schemas/runtime_request_v1.json"))
            .map_err(internal_error)?;
    let envelope_schema: Value =
        serde_json::from_str(include_str!("../../../schemas/runtime_envelope_v1.json"))
            .map_err(internal_error)?;
    Ok(Json(SchemaBundle {
        version: 1,
        request_schema,
        envelope_schema,
    }))
}

pub async fn privacy_handler() -> Json<PrivacyStatement> {
    Json(privacy_statement())
}

pub async fn agents_handler(
    State(state): State<LocalApiState>,
) -> Result<Json<AgentsResponse>, ProblemDetailsResponse> {
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
) -> Result<Json<DelegateResponse>, ProblemDetailsResponse> {
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
) -> Result<Json<WatchRollup>, ProblemDetailsResponse> {
    let snapshot = facade_watch_rollup(&state.config.working_dir, &id)
        .map_err(internal_anyhow)?
        .ok_or_else(|| not_found("task_not_found"))?;

    Ok(Json(WatchRollup {
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
) -> Result<Json<ControlResponse>, ProblemDetailsResponse> {
    let released =
        facade_release_session_task(&state.config.working_dir, &id).map_err(internal_anyhow)?;
    if released {
        if let Some(snapshot) =
            facade_get_session_task(&state.config.working_dir, &id).map_err(internal_anyhow)?
        {
            state.publish_session_task_rollup(snapshot);
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

/// Split a parent task into session tasks for a named team.
///
/// `POST /v1/teams/{team_name}/schedule` with body `{ parent_task_id, prompt }`
#[tracing::instrument(skip_all, fields(team_name = %team_name))]
pub async fn schedule_team_handler(
    State(state): State<LocalApiState>,
    Path(team_name): Path<String>,
    Json(request): Json<ScheduleTeamRequest>,
) -> Result<Json<ScheduleTeamResponse>, ProblemDetailsResponse> {
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
) -> Result<Json<JoinStatusResponse>, ProblemDetailsResponse> {
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
) -> Result<impl IntoResponse, ProblemDetailsResponse> {
    let RuntimeRequest::SubmitInput { task_id, input, .. } = request else {
        return Err(bad_request("invalid_request_type"));
    };

    let task_id = task_id.unwrap_or_else(new_server_task_id);
    let (envelope_tx, envelope_rx) = mpsc::unbounded_channel::<String>();
    let (interrupt_tx, interrupt_rx) = mpsc::unbounded_channel::<FrontendCommand>();
    let quit = Arc::new(AtomicBool::new(false));
    let shared = Arc::new(Mutex::new(LocalApiTaskShared::new(
        task_id.clone(),
        envelope_tx,
        Arc::clone(&quit),
        state
            .config
            .api_client
            .delta_accumulator_memory_watermark_bytes(),
    )));

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

    let mut response = runtime_sse_response(envelope_rx, SSE_KEEPALIVE_INTERVAL).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(SSE_CACHE_CONTROL_HEADER),
    );
    response.headers_mut().insert(
        HeaderName::from_static(SSE_PROXY_BUFFERING_HEADER),
        HeaderValue::from_static(SSE_PROXY_BUFFERING_DISABLED),
    );

    Ok(response)
}

pub async fn interrupt_handler(
    State(state): State<LocalApiState>,
    Json(request): Json<RuntimeRequest>,
) -> Result<Json<ControlResponse>, ProblemDetailsResponse> {
    let RuntimeRequest::Interrupt { task_id, .. } = request else {
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
) -> Result<Json<ControlResponse>, ProblemDetailsResponse> {
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

    if let RuntimeRequest::ApproveCapability { scope, .. } = &request
        && pending.scope != *scope
    {
        shared.pending_approval = Some(pending);
        return Err(conflict("no_pending_approval"));
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
    spawn(async move {
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
    let millis = Utc::now().timestamp_millis();
    format!("task-{millis}")
}

// ---------------------------------------------------------------------------
// Phase E — LocalApi session-task projection response types and handlers
// ---------------------------------------------------------------------------

pub(crate) mod session;
pub use self::session::*;
