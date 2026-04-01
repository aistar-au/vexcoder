use super::internal_anyhow;
use crate::app::{
    facade_get_session_task, facade_list_session_tasks, facade_list_tasks, facade_list_todos,
    facade_task_graph, facade_update_session_task_status, task_graph_snapshot_path,
    todos_snapshot_path, write_projection_snapshot, SessionTaskStatusError,
};
use crate::local_api::LocalApiState;
use crate::server::util::{bad_request, conflict, not_found};
use crate::server::{ControlResponse, SSE_KEEPALIVE_INTERVAL, SSE_KEEPALIVE_TEXT};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::convert::TryFrom;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::UnboundedReceiverStream;

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
/// snapshot is always emitted on connect.  Updates are fanned out through
/// `LocalApiState`'s in-process broadcast channel while the persisted
/// task-state files remain the durable source of truth for reconnects.  The
/// stream terminates automatically once the session task reaches a final
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

// ---------------------------------------------------------------------------
// Task graph — `GET /v1/task-graph`
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct TaskGraphNodeResponse {
    pub id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub session_tasks: Vec<SessionTaskSnapshotResponse>,
}

#[derive(Debug, Serialize)]
pub struct TaskGraphResponse {
    pub nodes: Vec<TaskGraphNodeResponse>,
}

/// Return the full task graph: every parent task node with its session tasks.
///
/// `GET /v1/task-graph`
#[tracing::instrument(skip_all)]
pub async fn task_graph_handler(
    State(state): State<LocalApiState>,
) -> Result<Json<TaskGraphResponse>, (StatusCode, Json<ControlResponse>)> {
    let graph = facade_task_graph(&state.config.working_dir).map_err(internal_anyhow)?;
    Ok(Json(TaskGraphResponse {
        nodes: graph
            .nodes
            .into_iter()
            .map(|n| TaskGraphNodeResponse {
                id: n.id,
                status: n.status,
                agent_id: n.agent_id,
                session_tasks: n
                    .session_tasks
                    .into_iter()
                    .map(snapshot_to_response)
                    .collect(),
            })
            .collect(),
    }))
}

// ---------------------------------------------------------------------------
// Todos — `GET /v1/todos`
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct TodoItemResponse {
    pub id: String,
    pub parent_task_id: String,
    pub agent_id: String,
    pub lifecycle_state: String,
}

/// Return all active (non-final) session tasks as a flat todo list.
///
/// `GET /v1/todos`
#[tracing::instrument(skip_all)]
pub async fn list_todos_handler(
    State(state): State<LocalApiState>,
) -> Result<Json<Vec<TodoItemResponse>>, (StatusCode, Json<ControlResponse>)> {
    let todos = facade_list_todos(&state.config.working_dir).map_err(internal_anyhow)?;
    Ok(Json(
        todos
            .into_iter()
            .map(|t| TodoItemResponse {
                id: t.id,
                parent_task_id: t.parent_task_id,
                agent_id: t.agent_id,
                lifecycle_state: t.lifecycle_state,
            })
            .collect(),
    ))
}

// ---------------------------------------------------------------------------
// Projection status — `GET /v1/projection`
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ProjectionStatusResponse {
    pub task_graph_path: String,
    pub todos_path: String,
    pub task_graph_written_ms: Option<u64>,
    pub todos_written_ms: Option<u64>,
}

fn file_modified_ms(path: &std::path::Path) -> Option<u64> {
    path.metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|d| u64::try_from(d.as_millis()).ok())
        })
}

/// Return the paths and last-write timestamps of the persistent projection
/// files, then refresh both files from the current task-state directory.
///
/// `GET /v1/projection`
#[tracing::instrument(skip_all)]
pub async fn projection_handler(
    State(state): State<LocalApiState>,
) -> Result<Json<ProjectionStatusResponse>, (StatusCode, Json<ControlResponse>)> {
    let working_dir = &state.config.working_dir;
    let graph_path = task_graph_snapshot_path(working_dir);
    let todos_path = todos_snapshot_path(working_dir);

    // Capture timestamps before the refresh so the response reflects the
    // previous write (useful for callers that want to detect staleness).
    let graph_written = file_modified_ms(&graph_path);
    let todos_written = file_modified_ms(&todos_path);

    // Refresh the files; non-fatal on error.
    if let Err(e) = write_projection_snapshot(working_dir) {
        tracing::warn!(error = ?e, "projection refresh failed during GET /v1/projection");
    }

    Ok(Json(ProjectionStatusResponse {
        task_graph_path: graph_path.display().to_string(),
        todos_path: todos_path.display().to_string(),
        task_graph_written_ms: graph_written,
        todos_written_ms: todos_written,
    }))
}
