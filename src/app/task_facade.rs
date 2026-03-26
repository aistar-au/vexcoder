use anyhow::Result;
use std::path::Path;
use thiserror::Error;

use crate::agents::{load_agents_config, IsolationPolicy};
use crate::runtime::{SessionTask, TaskState, WorktreeLeaseManager};

// ---------------------------------------------------------------------------
// Typed error for facade_delegate_session_task — replaces fragile string
// matching at the handler level (O-2).
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum DelegateError {
    #[error("agent_not_found")]
    AgentNotFound,
    #[error("agents_config_missing")]
    AgentsConfigMissing,
    #[error("parent_task_id_required")]
    ParentTaskIdRequired,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

// ---------------------------------------------------------------------------
// Facade return types — thin domain structs the transport layer maps to JSON.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FacadeAgentDescriptor {
    pub name: String,
    pub profile: String,
    pub isolation: String,
    pub live_session_tasks: usize,
}

#[derive(Debug, Clone)]
pub struct FacadeTeamDescriptor {
    pub name: String,
    pub members: Vec<String>,
    pub scheduler: String,
}

#[derive(Debug, Clone)]
pub struct FacadeAgentsListing {
    pub available: bool,
    pub agents: Vec<FacadeAgentDescriptor>,
    pub teams: Vec<FacadeTeamDescriptor>,
}

#[derive(Debug, Clone)]
pub struct FacadeDelegateResult {
    pub parent_task_id: String,
    pub session_task_id: String,
}

#[derive(Debug, Clone)]
pub struct FacadeWatchSnapshot {
    pub kind: &'static str,
    pub id: String,
    pub parent_task_id: Option<String>,
    pub agent_id: Option<String>,
    pub status: String,
    pub worktree_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Facade entrypoints — called by handlers, never by-passed.
// ---------------------------------------------------------------------------

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
                scheduler: format!("{:?}", team.scheduler),
            })
            .collect(),
    })
}

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

    let config = load_agents_config(working_dir)?;
    let Some(config) = config else {
        return Err(DelegateError::AgentsConfigMissing);
    };
    let Some(agent) = config.agent_profiles.iter().find(|a| a.name == agent_id) else {
        return Err(DelegateError::AgentNotFound);
    };

    let state_dir = TaskState::state_dir_from(working_dir);
    let mut parent_state = TaskState::load(&state_dir, &parent_task_id)
        .unwrap_or_else(|_| TaskState::new(parent_task_id.clone()));

    let mut session_task = SessionTask::new(parent_task_id.clone(), agent_id, prompt, None);

    if agent.isolation == IsolationPolicy::Worktree {
        let lease_manager = WorktreeLeaseManager::new(&state_dir);
        let lease = lease_manager.lease_for_task(&session_task.id, Some(&parent_task_id))?;
        session_task.worktree_path = Some(lease.path);
    }

    let session_task_id = session_task.id.clone();
    parent_state.add_session_task(session_task);
    parent_state.save(&state_dir)?;

    Ok(FacadeDelegateResult {
        parent_task_id,
        session_task_id,
    })
}

pub fn facade_watch_snapshot(working_dir: &Path, id: &str) -> Result<Option<FacadeWatchSnapshot>> {
    if let Ok(task_state) = TaskState::load_from_search_dirs_from(working_dir, id) {
        return Ok(Some(FacadeWatchSnapshot {
            kind: "task",
            id: task_state.id,
            parent_task_id: task_state.parent_task_id,
            agent_id: task_state.agent_id,
            status: format!("{:?}", task_state.status),
            worktree_path: task_state
                .worktree_path
                .as_ref()
                .map(|p| p.display().to_string()),
        }));
    }

    if let Some((parent_state, session_task)) =
        TaskState::find_session_task_in_saved_states(working_dir, id)?
    {
        return Ok(Some(FacadeWatchSnapshot {
            kind: "session-task",
            id: session_task.id,
            parent_task_id: Some(parent_state.id),
            agent_id: Some(session_task.agent_id),
            status: format!("{:?}", session_task.lifecycle_state),
            worktree_path: session_task
                .worktree_path
                .as_ref()
                .map(|p| p.display().to_string()),
        }));
    }

    Ok(None)
}
