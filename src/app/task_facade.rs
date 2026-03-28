use anyhow::Result;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use thiserror::Error;

use crate::agents::{load_agents_config, IsolationPolicy, TeamScheduler};
use crate::app::subtask_orchestrator::SubtaskOrchestrator;
use crate::runtime::{SessionTask, SessionTaskStatus, TaskState, WorktreeLeaseManager};

// ---------------------------------------------------------------------------
// Maximum prompt length accepted by facade_delegate_session_task and
// facade_schedule_team.  Guards against pathological inputs that would
// bloat persisted task-state payloads and request bodies carried through
// the operator surfaces.
// ---------------------------------------------------------------------------
const MAX_DELEGATE_PROMPT_BYTES: usize = 65_536;
const DELEGATE_LOCK_FILE_NAME: &str = ".delegate-session-task.lock";

fn team_scheduler_name(scheduler: TeamScheduler) -> &'static str {
    match scheduler {
        TeamScheduler::FanOutJoin => "fan_out_join",
        TeamScheduler::Sequential => "sequential",
    }
}

fn delegate_serialization_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn with_delegate_lock<T, E>(
    state_dir: &Path,
    operation: impl FnOnce() -> std::result::Result<T, E>,
) -> std::result::Result<T, E>
where
    E: From<anyhow::Error>,
{
    std::fs::create_dir_all(state_dir)
        .map_err(anyhow::Error::from)
        .map_err(E::from)?;

    let _in_process_guard = delegate_serialization_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let lock_file = open_delegate_lock_file(state_dir).map_err(E::from)?;
    lock_file
        .lock_exclusive()
        .map_err(anyhow::Error::from)
        .map_err(E::from)?;

    operation()
}

fn open_delegate_lock_file(state_dir: &Path) -> Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(state_dir.join(DELEGATE_LOCK_FILE_NAME))
        .map_err(anyhow::Error::from)
}

#[cfg(test)]
type DelegateRaceHook = std::sync::Arc<dyn Fn() + Send + Sync>;

#[cfg(test)]
fn delegate_race_hook_slot() -> &'static Mutex<Option<DelegateRaceHook>> {
    static HOOK: OnceLock<Mutex<Option<DelegateRaceHook>>> = OnceLock::new();
    HOOK.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
fn run_delegate_race_hook() {
    let hook = delegate_race_hook_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(not(test))]
fn run_delegate_race_hook() {}

#[cfg(test)]
struct DelegateRaceHookGuard;

#[cfg(test)]
impl Drop for DelegateRaceHookGuard {
    fn drop(&mut self) {
        *delegate_race_hook_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

#[cfg(test)]
fn install_delegate_race_hook(hook: DelegateRaceHook) -> DelegateRaceHookGuard {
    *delegate_race_hook_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(hook);
    DelegateRaceHookGuard
}

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
// Phase E facade types — LocalApi session-task projection
// ---------------------------------------------------------------------------

/// Summary of one persisted parent task returned by `facade_list_tasks`.
#[derive(Debug, Clone)]
pub struct FacadeTaskSummary {
    pub id: String,
    pub status: String,
    pub parent_task_id: Option<String>,
    pub agent_id: Option<String>,
    pub session_task_count: usize,
    pub live_session_task_count: usize,
}

/// Full projection of one session task returned by the listing and detail
/// endpoints.
#[derive(Debug, Clone)]
pub struct FacadeSessionTaskSnapshot {
    pub id: String,
    pub parent_task_id: String,
    pub agent_id: String,
    pub lifecycle_state: String,
    pub worktree_path: Option<String>,
    pub started_at_ms: Option<u64>,
    pub updated_at_ms: u64,
    pub handoff_summary: Option<String>,
}

/// Typed error for `facade_update_session_task_status`.
#[derive(Debug, Error)]
pub enum SessionTaskStatusError {
    #[error("session_task_not_found")]
    NotFound,
    #[error("invalid_status")]
    InvalidStatus,
    #[error("transition_not_allowed")]
    TransitionNotAllowed,
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
    pub max_parallel_tasks: u32,
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

    let state_dir = TaskState::state_dir_from(working_dir);
    let max_parallel_tasks = agent.max_parallel_tasks as usize;
    let isolation = agent.isolation;

    with_delegate_lock(&state_dir, || {
        // Per-agent concurrency enforcement (ADR-034 §1) must be uninterruptible
        // with session-task creation so concurrent callers cannot both observe
        // the same live-count snapshot and over-allocate a shared agent slot.
        let live_counts = TaskState::live_session_task_counts_from(working_dir)?;
        let live = *live_counts.get(agent_id).unwrap_or(&0);
        if live >= max_parallel_tasks {
            return Err(DelegateError::ConcurrencyLimitReached);
        }

        run_delegate_race_hook();

        let mut parent_state = TaskState::load(&state_dir, &parent_task_id)
            .unwrap_or_else(|_| TaskState::new(parent_task_id.clone()));

        let mut session_task = SessionTask::new(parent_task_id.clone(), agent_id, prompt, None);
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

        Ok(FacadeDelegateResult {
            parent_task_id: parent_task_id.clone(),
            session_task_id,
        })
    })
}

pub fn facade_watch_snapshot(working_dir: &Path, id: &str) -> Result<Option<FacadeWatchSnapshot>> {
    if let Ok(task_state) = TaskState::load_from_search_dirs_from(working_dir, id) {
        return Ok(Some(FacadeWatchSnapshot {
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
/// Returns `true` when the session task was found; `false` when no matching
/// session task exists in any saved task-state file.
///
/// If the task is in a live state it is transitioned to `Completed` before
/// the lease is released.  If the task is already in a terminal state
/// (`Completed`, `Failed`, `Cancelled`) the lease is released without
/// re-transitioning.
///
/// The caller must not re-use the worktree path after this call returns
/// successfully.
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
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    fn write_agents_toml(dir: &std::path::Path, content: &str) {
        let vex_dir = dir.join(".vex");
        std::fs::create_dir_all(&vex_dir).unwrap();
        std::fs::write(vex_dir.join("agents.toml"), content).unwrap();
    }

    // TaskState::state_dir_from consults VEX_STATE_DIR, so these tests must
    // not run concurrently with env-mutating tests elsewhere in the crate.
    fn env_lock() -> tokio::sync::MutexGuard<'static, ()> {
        crate::test_support::ENV_LOCK.blocking_lock()
    }

    #[test]
    fn delegate_rejects_prompt_exceeding_max_bytes() {
        let _env_lock = env_lock();
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
        let _env_lock = env_lock();
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
    fn delegate_enforces_max_parallel_tasks_under_parallel_calls() {
        let _env_lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        write_agents_toml(
            dir.path(),
            "[[agents]]\nname = \"worker\"\nisolation = \"shared\"\nmax_parallel_tasks = 1\n",
        );

        let _race_hook = install_delegate_race_hook(Arc::new(|| {
            thread::sleep(Duration::from_millis(150));
        }));

        let worker_count = 8;
        let barrier = Arc::new(Barrier::new(worker_count));
        let working_dir = dir.path().to_path_buf();
        let mut handles = Vec::new();

        for _ in 0..worker_count {
            let barrier = Arc::clone(&barrier);
            let working_dir = working_dir.clone();
            handles.push(thread::spawn(move || {
                barrier.wait();
                facade_delegate_session_task(
                    &working_dir,
                    Some("parent-race".to_string()),
                    "worker",
                    "inspect docs",
                )
            }));
        }

        let mut successes = 0;
        let mut limit_rejections = 0;
        for handle in handles {
            match handle.join().unwrap() {
                Ok(_) => successes += 1,
                Err(DelegateError::ConcurrencyLimitReached) => limit_rejections += 1,
                Err(other) => panic!("unexpected delegate result: {other:?}"),
            }
        }

        assert_eq!(
            successes, 1,
            "expected exactly one successful delegate call"
        );
        assert_eq!(
            limit_rejections,
            worker_count - 1,
            "expected remaining delegates to be rejected by the concurrency cap"
        );
    }

    #[test]
    fn release_transitions_live_task_to_completed_and_drops_lease() {
        let _env_lock = env_lock();
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
        let _env_lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::create_dir_all(TaskState::state_dir_from(dir.path())).unwrap();

        let result = facade_release_session_task(dir.path(), "nonexistent-task-id").unwrap();
        assert!(!result, "expected released = false for unknown id");
    }

    #[test]
    fn watch_snapshot_formats_parent_task_status_with_display_names() {
        let _env_lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        let state_dir = TaskState::state_dir_from(dir.path());
        std::fs::create_dir_all(&state_dir).unwrap();

        let mut parent = TaskState::new("parent-watch".to_string());
        parent.status = crate::runtime::TaskStatus::AwaitingApproval;
        parent.save(&state_dir).unwrap();

        let snapshot = facade_watch_snapshot(dir.path(), "parent-watch")
            .unwrap()
            .expect("expected parent-task snapshot");

        assert_eq!(snapshot.kind, "task");
        assert_eq!(snapshot.status, "awaiting_approval");
    }

    #[test]
    fn schedule_team_rejects_prompt_exceeding_max_bytes() {
        let _env_lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        write_agents_toml(
            dir.path(),
            "[[agents]]\nname = \"coder\"\nisolation = \"shared\"\nmax_parallel_tasks = 2\n\n[[teams]]\nname = \"review\"\nscheduler = \"fan_out\"\nagents = [\"coder\"]\n",
        );

        let long_prompt = "x".repeat(MAX_DELEGATE_PROMPT_BYTES + 1);
        let result = facade_schedule_team(dir.path(), "parent-1", "review", &long_prompt);

        assert!(
            matches!(result, Err(ScheduleTeamError::PromptTooLong)),
            "expected PromptTooLong, got: {result:?}"
        );
    }

    #[test]
    fn schedule_team_rejects_empty_parent_task_id() {
        let _env_lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        let result = facade_schedule_team(dir.path(), "", "review", "do something");
        assert!(
            matches!(result, Err(ScheduleTeamError::ParentTaskIdRequired)),
            "expected ParentTaskIdRequired, got: {result:?}"
        );
    }

    #[test]
    fn schedule_team_rejects_empty_prompt() {
        let _env_lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        let result = facade_schedule_team(dir.path(), "parent-1", "review", "");
        assert!(
            matches!(result, Err(ScheduleTeamError::PromptRequired)),
            "expected PromptRequired, got: {result:?}"
        );
    }

    #[test]
    fn schedule_team_enforces_max_parallel_tasks() {
        let _env_lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        write_agents_toml(
            dir.path(),
            "[[agents]]\nname = \"coder\"\nisolation = \"shared\"\nmax_parallel_tasks = 1\n\n[[teams]]\nname = \"review\"\nscheduler = \"fan_out_join\"\nmembers = [\"coder\"]\n",
        );

        let state_dir = TaskState::state_dir_from(dir.path());
        std::fs::create_dir_all(&state_dir).unwrap();
        let mut parent = TaskState::new("parent-seed".to_string());
        parent.add_session_task(SessionTask::new(
            "parent-seed",
            "coder",
            "already running",
            None,
        ));
        parent.save(&state_dir).unwrap();

        let result = facade_schedule_team(dir.path(), "parent-1", "review", "new work");

        assert!(
            matches!(result, Err(ScheduleTeamError::ConcurrencyLimitReached)),
            "expected ConcurrencyLimitReached, got: {result:?}"
        );
    }

    #[test]
    fn schedule_team_enforces_max_parallel_tasks_under_parallel_calls() {
        let _env_lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        write_agents_toml(
            dir.path(),
            "[[agents]]\nname = \"coder\"\nisolation = \"shared\"\nmax_parallel_tasks = 1\n\n[[teams]]\nname = \"review\"\nscheduler = \"fan_out_join\"\nmembers = [\"coder\"]\n",
        );

        let _race_hook = install_delegate_race_hook(Arc::new(|| {
            thread::sleep(Duration::from_millis(150));
        }));

        let worker_count = 8;
        let barrier = Arc::new(Barrier::new(worker_count));
        let working_dir = dir.path().to_path_buf();
        let mut handles = Vec::new();

        for _ in 0..worker_count {
            let barrier = Arc::clone(&barrier);
            let working_dir = working_dir.clone();
            handles.push(thread::spawn(move || {
                barrier.wait();
                facade_schedule_team(&working_dir, "parent-race", "review", "inspect docs")
            }));
        }

        let mut successes = 0;
        let mut limit_rejections = 0;
        for handle in handles {
            match handle.join().unwrap() {
                Ok(_) => successes += 1,
                Err(ScheduleTeamError::ConcurrencyLimitReached) => limit_rejections += 1,
                Err(other) => panic!("unexpected schedule_team result: {other:?}"),
            }
        }

        assert_eq!(
            successes, 1,
            "expected exactly one successful schedule_team call"
        );
        assert_eq!(
            limit_rejections,
            worker_count - 1,
            "expected remaining schedule_team calls to be rejected by the concurrency cap"
        );
    }

    #[test]
    fn list_agents_exposes_max_parallel_tasks() {
        let _env_lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        // worktree isolation allows max_parallel_tasks > 1.
        write_agents_toml(
            dir.path(),
            "[[agents]]\nname = \"coder\"\nisolation = \"worktree\"\nmax_parallel_tasks = 3\n",
        );

        let listing = facade_list_agents(dir.path()).unwrap();
        assert!(listing.available, "listing should be available");
        let agent = &listing.agents[0];
        assert_eq!(agent.name, "coder");
        assert_eq!(agent.max_parallel_tasks, 3);
        assert_eq!(agent.isolation, "worktree");
    }

    #[test]
    fn list_agents_normalizes_team_scheduler_to_snake_case() {
        let _env_lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        write_agents_toml(
            dir.path(),
            "[[agents]]\nname = \"a\"\nisolation = \"shared\"\nmax_parallel_tasks = 1\n\n[[teams]]\nname = \"t\"\nscheduler = \"fan_out_join\"\nmembers = [\"a\"]\n",
        );

        let listing = facade_list_agents(dir.path()).unwrap();
        assert_eq!(listing.teams[0].scheduler, "fan_out_join");
    }

    #[test]
    fn schedule_team_normalizes_scheduler_to_snake_case() {
        let _env_lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        write_agents_toml(
            dir.path(),
            "[[agents]]\nname = \"coder\"\nisolation = \"shared\"\nmax_parallel_tasks = 1\n\n[[teams]]\nname = \"review\"\nscheduler = \"fan_out_join\"\nmembers = [\"coder\"]\n",
        );

        let result = facade_schedule_team(dir.path(), "parent-1", "review", "inspect docs")
            .expect("team scheduling should succeed");

        assert_eq!(result.scheduler, "fan_out_join");
    }

    #[test]
    fn schedule_team_returns_internal_for_unknown_member_reference() {
        let _env_lock = env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let vex_dir = dir.path().join(".vex");
        std::fs::create_dir_all(&vex_dir).unwrap();
        std::fs::write(
            vex_dir.join("agents.toml"),
            "[[agents]]\nname = \"coder\"\nisolation = \"shared\"\nmax_parallel_tasks = 1\n\n[[teams]]\nname = \"review\"\nscheduler = \"fan_out_join\"\nmembers = [\"missing\"]\n",
        )
        .unwrap();

        let result = facade_schedule_team(dir.path(), "parent-1", "review", "inspect docs");

        match result {
            Err(ScheduleTeamError::Internal(error)) => {
                let message = error.to_string();
                assert!(
                    message.contains("unknown agent"),
                    "unexpected internal error message: {message}"
                );
            }
            other => panic!("expected Internal error, got: {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Phase C facade entrypoints — subtask orchestration
// ---------------------------------------------------------------------------

/// Result returned by `facade_schedule_team`.
#[derive(Debug, Clone)]
pub struct FacadeScheduleTeamResult {
    pub parent_task_id: String,
    /// IDs of session tasks created in this call.
    pub session_task_ids: Vec<String>,
    /// Scheduler used: `"fan_out_join"` or `"sequential"`.
    pub scheduler: String,
}

/// Fan-out status snapshot returned by `facade_poll_join`.
#[derive(Debug, Clone)]
pub struct FacadeJoinOutcome {
    pub all_done: bool,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
    /// `(agent_id, summary)` pairs from tasks that recorded a handoff summary.
    pub summaries: Vec<(String, String)>,
}

/// Decompose a parent task into session tasks for a named team.
///
/// `team_name` must match a `[[teams]]` entry in `.vex/agents.toml`.
/// Returns `Err` with `agents_config_missing` when no config is found,
/// `team_not_found` when the team name does not match, and `Internal` for
/// unexpected I/O errors.
pub fn facade_schedule_team(
    working_dir: &Path,
    parent_task_id: &str,
    team_name: &str,
    prompt: &str,
) -> Result<FacadeScheduleTeamResult, ScheduleTeamError> {
    if parent_task_id.trim().is_empty() {
        return Err(ScheduleTeamError::ParentTaskIdRequired);
    }
    if prompt.is_empty() {
        return Err(ScheduleTeamError::PromptRequired);
    }
    if prompt.len() > MAX_DELEGATE_PROMPT_BYTES {
        return Err(ScheduleTeamError::PromptTooLong);
    }

    let config = load_agents_config(working_dir)?;
    let Some(config) = config else {
        return Err(ScheduleTeamError::AgentsConfigMissing);
    };
    let Some(team) = config.team_definitions.iter().find(|t| t.name == team_name) else {
        return Err(ScheduleTeamError::TeamNotFound);
    };

    let state_dir = TaskState::state_dir_from(working_dir);
    with_delegate_lock(&state_dir, || {
        let members_to_create: &[String] = match team.scheduler {
            crate::agents::TeamScheduler::FanOutJoin => &team.members,
            crate::agents::TeamScheduler::Sequential => &team.members[..1],
        };

        let live_counts = TaskState::live_session_task_counts_from(working_dir)?;
        for member_name in members_to_create {
            let agent = config
                .agent_profiles
                .iter()
                .find(|agent| agent.name == *member_name)
                .ok_or_else(|| {
                    ScheduleTeamError::Internal(anyhow::anyhow!(
                        "team '{}' references unknown agent member '{}'",
                        team_name,
                        member_name
                    ))
                })?;
            let live = *live_counts.get(member_name).unwrap_or(&0);
            if live >= agent.max_parallel_tasks as usize {
                return Err(ScheduleTeamError::ConcurrencyLimitReached);
            }
        }

        run_delegate_race_hook();

        let orchestrator = SubtaskOrchestrator::new(&state_dir);
        let decomp =
            orchestrator.schedule_team(parent_task_id, team, &config.agent_profiles, prompt)?;

        Ok(FacadeScheduleTeamResult {
            parent_task_id: decomp.parent_task_id,
            session_task_ids: decomp.session_task_ids,
            scheduler: team_scheduler_name(decomp.scheduler).to_string(),
        })
    })
}

/// Check the fan-out join gate for a parent task.
///
/// Returns `Ok(None)` when at least one session task is still live.  Returns
/// `Ok(Some(FacadeJoinOutcome))` when every session task has reached a
/// terminal state.  When the outcome is returned, the caller should call the
/// apply-join endpoint to persist the merged handoff summary.
pub fn facade_poll_join(
    working_dir: &Path,
    parent_task_id: &str,
) -> Result<Option<FacadeJoinOutcome>> {
    let state_dir = TaskState::state_dir_from(working_dir);
    let orchestrator = SubtaskOrchestrator::new(&state_dir);
    let outcome = orchestrator.poll_fan_out_join(parent_task_id)?;
    Ok(outcome.map(|o| FacadeJoinOutcome {
        all_done: o.all_done,
        completed: o.completed,
        failed: o.failed,
        cancelled: o.cancelled,
        summaries: o.summaries,
    }))
}

/// Typed error for `facade_schedule_team`.
#[derive(Debug, Error)]
pub enum ScheduleTeamError {
    #[error("agents_config_missing")]
    AgentsConfigMissing,
    #[error("team_not_found")]
    TeamNotFound,
    #[error("parent_task_id_required")]
    ParentTaskIdRequired,
    #[error("prompt_required")]
    PromptRequired,
    #[error("concurrency_limit_reached")]
    ConcurrencyLimitReached,
    #[error("prompt_too_long")]
    PromptTooLong,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

// ---------------------------------------------------------------------------
// Phase E facade entrypoints — LocalApi session-task projection
// ---------------------------------------------------------------------------

/// Return a summary for every persisted parent-task state file.
pub fn facade_list_tasks(working_dir: &Path) -> Result<Vec<FacadeTaskSummary>> {
    let mut out = Vec::new();
    for file in TaskState::state_files_from(working_dir) {
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

/// Return a snapshot for every session task across all persisted parent states.
pub fn facade_list_session_tasks(working_dir: &Path) -> Result<Vec<FacadeSessionTaskSnapshot>> {
    let mut out = Vec::new();
    for file in TaskState::state_files_from(working_dir) {
        let state = TaskState::load(&file.dir, &file.id)?;
        for task in state.session_tasks {
            out.push(session_task_to_snapshot(task));
        }
    }
    Ok(out)
}

/// Return the snapshot for a single session task identified by its UUID.
pub fn facade_get_session_task(
    working_dir: &Path,
    session_task_id: &str,
) -> Result<Option<FacadeSessionTaskSnapshot>> {
    let Some((_, task)) =
        TaskState::find_session_task_in_saved_states(working_dir, session_task_id)?
    else {
        return Ok(None);
    };
    Ok(Some(session_task_to_snapshot(task)))
}

/// Transition a session task to a new lifecycle state reported by an external
/// agent.
///
/// Accepted `status_str` values: `running`, `blocked`, `failed`,
/// `cancelled`, `completed`.
///
/// Transitions from terminal states (`Failed`, `Cancelled`, `Completed`) are
/// rejected with `TransitionNotAllowed` — use
/// `facade_release_session_task` to clean up a terminal task instead.
pub fn facade_update_session_task_status(
    working_dir: &Path,
    session_task_id: &str,
    status_str: &str,
) -> std::result::Result<FacadeSessionTaskSnapshot, SessionTaskStatusError> {
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

    Ok(session_task_to_snapshot(updated))
}

fn session_task_to_snapshot(task: SessionTask) -> FacadeSessionTaskSnapshot {
    FacadeSessionTaskSnapshot {
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
