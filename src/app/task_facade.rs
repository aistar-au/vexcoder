use anyhow::Result;
use std::path::Path;
use thiserror::Error;

use crate::agents::{load_agents_config, IsolationPolicy};
use crate::runtime::{SessionTask, SessionTaskStatus, TaskState, WorktreeLeaseManager};

// ---------------------------------------------------------------------------
// Maximum prompt length accepted by facade_delegate_session_task.
// Guards against pathological inputs that would bloat persisted task-state
// payloads and request bodies carried through the operator surfaces.
// ---------------------------------------------------------------------------
const MAX_DELEGATE_PROMPT_BYTES: usize = 65_536;

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
    /// The agent's `max_parallel_tasks` limit is already reached.
    ///
    /// ADR-034 §1 requires the orchestrator to enforce per-agent concurrency
    /// limits at delegation time.  The caller must wait for a live session task
    /// to complete before retrying.
    #[error("concurrency_limit_reached")]
    ConcurrencyLimitReached,
    /// The supplied prompt exceeds `MAX_DELEGATE_PROMPT_BYTES`.
    #[error("prompt_too_long")]
    PromptTooLong,
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

    // Prompt length guard — prevents pathological inputs.
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

    // Per-agent concurrency enforcement (ADR-034 §1).
    // The live count is computed inline from saved task-state files rather
    // than from a cached counter so that the check remains accurate across
    // process restarts and concurrent callers.
    let live_counts = TaskState::live_session_task_counts_from(working_dir)?;
    let live = *live_counts.get(agent_id).unwrap_or(&0);
    if live >= agent.max_parallel_tasks as usize {
        return Err(DelegateError::ConcurrencyLimitReached);
    }

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
            status: session_task.lifecycle_state.to_string(),
            worktree_path: session_task
                .worktree_path
                .as_ref()
                .map(|p| p.display().to_string()),
        }));
    }

    Ok(None)
}

/// Mark a session task as `Completed` and release its worktree lease.
///
/// Returns `true` when the session task was found and transitioned; `false`
/// when no matching session task exists in any saved task-state file.
///
/// The caller must not re-use the worktree path after this call returns
/// successfully.  If the session task was already in a terminal state
/// (`Completed`, `Failed`, `Cancelled`) the lease is released and the
/// function returns `true` without re-transitioning.
pub fn facade_release_session_task(working_dir: &Path, session_task_id: &str) -> Result<bool> {
    let state_dir = TaskState::state_dir_from(working_dir);

    let Some((mut parent_state, _session_task)) =
        TaskState::find_session_task_in_saved_states(working_dir, session_task_id)?
    else {
        return Ok(false);
    };

    // Transition to Completed only when currently live.
    if parent_state
        .session_task(session_task_id)
        .map(|t| t.lifecycle_state.is_live())
        .unwrap_or(false)
    {
        parent_state.update_session_task_status(session_task_id, SessionTaskStatus::Completed);
        parent_state.save(&state_dir)?;
    }

    // Release the worktree lease regardless of prior lifecycle state so that
    // stale leases from interrupted processes are cleaned up on explicit release.
    let lease_manager = WorktreeLeaseManager::new(&state_dir);
    if lease_manager.load(session_task_id).is_ok() {
        lease_manager.release(session_task_id)?;
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::SessionTask;
    use std::path::PathBuf;

    fn write_agents_toml(dir: &std::path::Path, content: &str) {
        let vex_dir = dir.join(".vex");
        std::fs::create_dir_all(&vex_dir).unwrap();
        std::fs::write(vex_dir.join("agents.toml"), content).unwrap();
    }

    #[test]
    fn delegate_rejects_prompt_exceeding_max_bytes() {
        let dir = tempfile::tempdir().unwrap();
        write_agents_toml(
            dir.path(),
            "[[agents]]\nname = \"worker\"\nisolation = \"shared\"\nmax_parallel_tasks = 2\n",
        );

        let long_prompt = "x".repeat(MAX_DELEGATE_PROMPT_BYTES + 1);
        let result = facade_delegate_session_task(
            dir.path(),
            Some("parent-1".to_string()),
            "worker",
            &long_prompt,
        );

        assert!(
            matches!(result, Err(DelegateError::PromptTooLong)),
            "expected PromptTooLong, got: {result:?}"
        );
    }

    #[test]
    fn delegate_enforces_max_parallel_tasks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        write_agents_toml(
            dir.path(),
            "[[agents]]\nname = \"worker\"\nisolation = \"shared\"\nmax_parallel_tasks = 1\n",
        );

        // Seed a live session task for the same agent so the limit is already
        // at capacity before the next delegate call.
        let state_dir = TaskState::state_dir_from(dir.path());
        std::fs::create_dir_all(&state_dir).unwrap();
        let mut parent = TaskState::new("parent-seed".to_string());
        let mut st = SessionTask::new("parent-seed", "worker", "already running", None);
        // Leave lifecycle_state as Pending (which is_live() == true).
        st.worktree_path = Some(PathBuf::from("/tmp/dummy"));
        parent.add_session_task(st);
        parent.save(&state_dir).unwrap();

        let result = facade_delegate_session_task(
            dir.path(),
            Some("parent-1".to_string()),
            "worker",
            "new work",
        );

        assert!(
            matches!(result, Err(DelegateError::ConcurrencyLimitReached)),
            "expected ConcurrencyLimitReached, got: {result:?}"
        );
    }

    #[test]
    fn release_transitions_live_task_to_completed_and_drops_lease() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();

        let state_dir = TaskState::state_dir_from(dir.path());
        std::fs::create_dir_all(&state_dir).unwrap();

        let mut parent = TaskState::new("parent-rel".to_string());
        let mut st = SessionTask::new("parent-rel", "reviewer", "review docs", None);
        let st_id = st.id.clone();
        let lease_manager = WorktreeLeaseManager::new(&state_dir);
        let lease = lease_manager
            .lease_for_task(&st_id, Some("parent-rel"))
            .unwrap();
        st.worktree_path = Some(lease.path.clone());
        parent.add_session_task(st);
        parent.save(&state_dir).unwrap();

        let released = facade_release_session_task(dir.path(), &st_id).unwrap();
        assert!(released, "expected released = true");

        // Reload and verify transition.
        let reloaded = TaskState::load(&state_dir, "parent-rel").unwrap();
        let task = reloaded.session_task(&st_id).unwrap();
        assert_eq!(task.lifecycle_state, SessionTaskStatus::Completed);
        assert!(!lease.path.exists(), "expected lease path to be removed");
        assert!(
            lease_manager.list().unwrap().is_empty(),
            "expected lease metadata to be removed"
        );
    }

    #[test]
    fn release_returns_false_for_unknown_session_task_id() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::create_dir_all(TaskState::state_dir_from(dir.path())).unwrap();

        let result = facade_release_session_task(dir.path(), "nonexistent-task-id").unwrap();
        assert!(!result, "expected released = false for unknown id");
    }
}
