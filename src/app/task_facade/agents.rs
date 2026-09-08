use anyhow::Result;
use std::path::Path;

use crate::agents::{IsolationPolicy, load_agents_config};
use crate::runtime::{SessionTask, SessionTaskStatus, TaskState, WorktreeLeaseManager};

use super::projection::write_projection_rollup;
use super::types::{
    FacadeAgentDescriptor, FacadeAgentsListing, FacadeDelegateResult, FacadeTeamDescriptor,
    FacadeWatchRollup,
};
use super::{
    DelegateError, run_delegate_race_hook, team_scheduler_name, with_delegate_lock,
    MAX_DELEGATE_PROMPT_BYTES,
};

#[tracing::instrument(skip(working_dir), fields(working_dir = %working_dir.display()))]
pub fn facade_list_agents(working_dir: &Path) -> Result<FacadeAgentsListing> {
    let config = load_agents_config(working_dir)?;
    let Some(config) = config else {
        return Ok(FacadeAgentsListing {
            available: false,
            agents: Vec::new(),
            teams: Vec::new(),
        });
    };

    let mut live_counts = TaskState::live_session_task_counts_from(working_dir)?;

    Ok(FacadeAgentsListing {
        available: true,
        agents: config
            .agent_profiles
            .into_iter()
            .map(|agent| FacadeAgentDescriptor {
                live_session_tasks: live_counts.remove(&agent.name).unwrap_or_default(),
                max_parallel_tasks: agent.max_parallel_tasks,
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
            .map(|team| FacadeTeamDescriptor {
                name: team.name,
                members: team.members,
                scheduler: team_scheduler_name(team.scheduler).to_string(),
            })
            .collect(),
    })
}

#[tracing::instrument(skip(working_dir, prompt), fields(working_dir = %working_dir.display()))]
pub fn facade_delegate_session_task(
    working_dir: &Path,
    parent_task_id: Option<String>,
    agent_id: &str,
    prompt: &str,
) -> std::result::Result<FacadeDelegateResult, DelegateError> {
    let parent_task_id = match parent_task_id {
        Some(id) if !id.trim().is_empty() => id,
        _ => return Err(DelegateError::ParentTaskIdRequired),
    };

    if prompt.len() > MAX_DELEGATE_PROMPT_BYTES {
        return Err(DelegateError::PromptTooLong);
    }

    let config = load_agents_config(working_dir)?;
    let Some(config) = config else {
        return Err(DelegateError::AgentsConfigMissing);
    };
    let Some(agent) = config.agent_profiles.iter().find(|a| a.name == agent_id) else {
        return Err(DelegateError::AgentNotFound);
    };

    let state_dir = TaskState::state_dir_from(working_dir);
    let max_parallel_tasks = agent.max_parallel_tasks as usize;
    let isolation = agent.isolation;

    with_delegate_lock(&state_dir, || {
        let live_counts = TaskState::live_session_task_counts_from(working_dir)?;
        let live = *live_counts.get(agent_id).unwrap_or(&0);
        if live >= max_parallel_tasks {
            return Err(DelegateError::ConcurrencyLimitReached);
        }

        run_delegate_race_hook();

        let mut parent_state = TaskState::load(&state_dir, &parent_task_id)
            .unwrap_or_else(|_| TaskState::new(parent_task_id.clone()));

        let mut session_task = SessionTask::new(parent_task_id.clone(), agent_id, prompt, None);
        session_task.stamp_join_supersedes(&parent_state.session_tasks, false);
        let session_task_id = session_task.id.clone();

        if isolation == IsolationPolicy::Worktree {
            let lease_manager = WorktreeLeaseManager::new(&state_dir);
            let lease = lease_manager.lease_for_task(&session_task_id, Some(&parent_task_id))?;
            session_task.worktree_path = Some(lease.path);
        }

        parent_state.add_session_task(session_task);
        if let Err(error) = parent_state.save(&state_dir) {
            if isolation == IsolationPolicy::Worktree {
                let lease_manager = WorktreeLeaseManager::new(&state_dir);
                if let Err(lease_err) = lease_manager.release(&session_task_id) {
                    tracing::error!(
                        session_task_id = %session_task_id,
                        save_error = ?error,
                        lease_error = ?lease_err,
                        "failed to save parent state and release lease",
                    );
                }
            }
            return Err(DelegateError::Internal(error));
        }

        let result = Ok(FacadeDelegateResult {
            parent_task_id: parent_task_id.clone(),
            session_task_id,
        });
        if let Err(e) = write_projection_rollup(working_dir) {
            tracing::warn!(error = ?e, "failed to write projection rollup after delegate");
        }
        result
    })
}

#[tracing::instrument(skip(working_dir), fields(working_dir = %working_dir.display()))]
pub fn facade_watch_rollup(working_dir: &Path, id: &str) -> Result<Option<FacadeWatchRollup>> {
    if let Ok(task_state) = TaskState::load_from_search_dirs_from(working_dir, id) {
        return Ok(Some(FacadeWatchRollup {
            kind: "task",
            id: task_state.id,
            parent_task_id: task_state.parent_task_id,
            agent_id: task_state.agent_id,
            status: task_state.status.to_string(),
            worktree_path: task_state
                .worktree_path
                .as_ref()
                .map(|p| p.display().to_string()),
        }));
    }

    if let Some((parent_state, session_task)) =
        TaskState::find_session_task_in_saved_states(working_dir, id)?
    {
        return Ok(Some(FacadeWatchRollup {
            kind: "session-task",
            id: session_task.id,
            parent_task_id: Some(parent_state.id),
            agent_id: Some(session_task.agent_id),
            status: session_task.lifecycle_state.to_string(),
            worktree_path: session_task
                .worktree_path
                .as_ref()
                .map(|p| p.display().to_string()),
        }));
    }

    Ok(None)
}

#[tracing::instrument(skip(working_dir), fields(working_dir = %working_dir.display()))]
pub fn facade_release_session_task(working_dir: &Path, session_task_id: &str) -> Result<bool> {
    let state_dir = TaskState::state_dir_from(working_dir);

    let Some((mut parent_state, _session_task)) =
        TaskState::find_session_task_in_saved_states(working_dir, session_task_id)?
    else {
        return Ok(false);
    };

    if parent_state
        .session_task(session_task_id)
        .map(|t| t.lifecycle_state.is_live())
        .unwrap_or(false)
    {
        parent_state.update_session_task_status(session_task_id, SessionTaskStatus::Completed);
        parent_state.save(&state_dir)?;
    }

    let lease_manager = WorktreeLeaseManager::new(&state_dir);
    if lease_manager.load(session_task_id).is_ok() {
        lease_manager.release(session_task_id)?;
    }

    if let Err(e) = write_projection_rollup(working_dir) {
        tracing::warn!(error = ?e, "failed to write projection rollup after release");
    }

    Ok(true)
}
