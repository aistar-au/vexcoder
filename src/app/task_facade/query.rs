use anyhow::Result;
use std::path::Path;

use crate::runtime::{SessionTask, SessionTaskStatus, TaskState};

use super::projection::write_projection_rollup;
use super::types::{
    FacadeSessionTaskRollup, FacadeTaskGraph, FacadeTaskGraphNode, FacadeTaskSummary,
    FacadeTodoItem, SessionTaskStatusError,
};

#[tracing::instrument(skip(working_dir), fields(working_dir = %working_dir.display()))]
pub fn facade_list_tasks(working_dir: &Path) -> Result<Vec<FacadeTaskSummary>> {
    let files = TaskState::state_files_from(working_dir);
    let mut out = Vec::with_capacity(files.len());
    for file in files {
        let state = TaskState::load(&file.dir, &file.id)?;
        let live = state
            .session_tasks
            .iter()
            .filter(|t| t.lifecycle_state.is_live())
            .count();
        out.push(FacadeTaskSummary {
            id: state.id,
            status: state.status.to_string(),
            parent_task_id: state.parent_task_id,
            agent_id: state.agent_id,
            session_task_count: state.session_tasks.len(),
            live_session_task_count: live,
        });
    }
    Ok(out)
}

#[tracing::instrument(skip(working_dir), fields(working_dir = %working_dir.display()))]
pub fn facade_list_session_tasks(working_dir: &Path) -> Result<Vec<FacadeSessionTaskRollup>> {
    let files = TaskState::state_files_from(working_dir);
    let mut out = Vec::with_capacity(files.len());
    for file in files {
        let state = TaskState::load(&file.dir, &file.id)?;
        for task in state.session_tasks {
            out.push(session_task_to_rollup(task));
        }
    }
    Ok(out)
}

#[tracing::instrument(skip(working_dir), fields(working_dir = %working_dir.display()))]
pub fn facade_get_session_task(
    working_dir: &Path,
    session_task_id: &str,
) -> Result<Option<FacadeSessionTaskRollup>> {
    let Some((_, task)) =
        TaskState::find_session_task_in_saved_states(working_dir, session_task_id)?
    else {
        return Ok(None);
    };
    Ok(Some(session_task_to_rollup(task)))
}

#[tracing::instrument(skip(working_dir), fields(working_dir = %working_dir.display()))]
pub fn facade_update_session_task_status(
    working_dir: &Path,
    session_task_id: &str,
    status_str: &str,
) -> std::result::Result<FacadeSessionTaskRollup, SessionTaskStatusError> {
    let new_status =
        parse_session_task_status(status_str).ok_or(SessionTaskStatusError::InvalidStatus)?;

    let state_dir = TaskState::state_dir_from(working_dir);

    let Some((mut parent_state, existing)) =
        TaskState::find_session_task_in_saved_states(working_dir, session_task_id)?
    else {
        return Err(SessionTaskStatusError::NotFound);
    };

    if !existing.lifecycle_state.is_live() {
        return Err(SessionTaskStatusError::TransitionNotAllowed);
    }

    parent_state.update_session_task_status(session_task_id, new_status);
    parent_state.save(&state_dir)?;

    let updated = parent_state
        .session_task(session_task_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("session task missing after save"))?;

    let snapshot = session_task_to_rollup(updated);
    if let Err(e) = write_projection_rollup(working_dir) {
        tracing::warn!(error = ?e, "failed to write projection rollup after status update");
    }
    Ok(snapshot)
}

#[tracing::instrument(skip(working_dir), fields(working_dir = %working_dir.display()))]
pub fn facade_task_graph(working_dir: &Path) -> Result<FacadeTaskGraph> {
    let files = TaskState::state_files_from(working_dir);
    let mut nodes = Vec::with_capacity(files.len());
    for file in files {
        let state = TaskState::load(&file.dir, &file.id)?;
        let session_tasks = state
            .session_tasks
            .into_iter()
            .map(session_task_to_rollup)
            .collect();
        nodes.push(FacadeTaskGraphNode {
            id: state.id,
            status: state.status.to_string(),
            agent_id: state.agent_id,
            session_tasks,
        });
    }
    Ok(FacadeTaskGraph { nodes })
}

#[tracing::instrument(skip(working_dir), fields(working_dir = %working_dir.display()))]
pub fn facade_list_todos(working_dir: &Path) -> Result<Vec<FacadeTodoItem>> {
    let files = TaskState::state_files_from(working_dir);
    let mut out = Vec::with_capacity(files.len());
    for file in files {
        let state = TaskState::load(&file.dir, &file.id)?;
        for task in state.session_tasks {
            if task.lifecycle_state.is_live() {
                out.push(FacadeTodoItem {
                    id: task.id,
                    parent_task_id: task.parent_task_id,
                    agent_id: task.agent_id,
                    lifecycle_state: task.lifecycle_state.to_string(),
                });
            }
        }
    }
    Ok(out)
}

fn session_task_to_rollup(task: SessionTask) -> FacadeSessionTaskRollup {
    FacadeSessionTaskRollup {
        lifecycle_state: task.lifecycle_state.to_string(),
        worktree_path: task.worktree_path.as_ref().map(|p| p.display().to_string()),
        started_at_ms: task.started_at,
        updated_at_ms: task.updated_at,
        handoff_summary: task.handoff_summary,
        id: task.id,
        parent_task_id: task.parent_task_id,
        agent_id: task.agent_id,
    }
}

fn parse_session_task_status(s: &str) -> Option<SessionTaskStatus> {
    match s {
        "pending" => Some(SessionTaskStatus::Pending),
        "running" => Some(SessionTaskStatus::Running),
        "blocked" => Some(SessionTaskStatus::Blocked),
        "failed" => Some(SessionTaskStatus::Failed),
        "cancelled" => Some(SessionTaskStatus::Cancelled),
        "completed" => Some(SessionTaskStatus::Completed),
        _ => None,
    }
}
