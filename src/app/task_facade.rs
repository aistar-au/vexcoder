use anyhow::Result;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use thiserror::Error;

use crate::agents::{IsolationPolicy, TeamScheduler, load_agents_config};
use crate::app::subtask_orchestrator::SubtaskOrchestrator;
use crate::runtime::{SessionTask, SessionTaskStatus, TaskState, WorktreeLeaseManager};

pub mod projection;
#[cfg(test)]
mod tests;
mod types;

pub use self::projection::{task_graph_rollup_path, todos_rollup_path, write_projection_rollup};
pub use self::types::{
    FacadeAgentDescriptor, FacadeAgentsListing, FacadeDelegateResult, FacadeJoinOutcome,
    FacadeScheduleTeamResult, FacadeSessionTaskRollup, FacadeTaskGraph, FacadeTaskGraphNode,
    FacadeTaskSummary, FacadeTeamDescriptor, FacadeTodoItem, FacadeWatchRollup, PeerChannelError,
    ScheduleTeamError, SessionTaskStatusError,
};

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
    /// limits at delegation time.  The caller must wait for an active session task
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
// Facade entrypoints — called by handlers, never by-passed.
// ---------------------------------------------------------------------------

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
        // the same active-count snapshot and over-allocate a shared agent slot.
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

/// Mark a session task as `Completed` and release its worktree lease.
///
/// Returns `true` when the session task was found; `false` when no matching
/// session task exists in any saved task-state file.
///
/// If the task is in an active (non-final) state it is transitioned to `Completed` before
/// the lease is released.  If the task is already in a final state
/// (`Completed`, `Failed`, `Cancelled`) the lease is released without
/// re-transitioning.
///
/// The caller must not re-use the worktree path after this call returns
/// successfully.
#[tracing::instrument(skip(working_dir), fields(working_dir = %working_dir.display()))]
pub fn facade_release_session_task(working_dir: &Path, session_task_id: &str) -> Result<bool> {
    let state_dir = TaskState::state_dir_from(working_dir);

    let Some((mut parent_state, _session_task)) =
        TaskState::find_session_task_in_saved_states(working_dir, session_task_id)?
    else {
        return Ok(false);
    };

    // Transition to Completed only when currently in a non-final state.
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

    if let Err(e) = write_projection_rollup(working_dir) {
        tracing::warn!(error = ?e, "failed to write projection rollup after release");
    }

    Ok(true)
}

// ---------------------------------------------------------------------------
// Phase C facade entrypoints — subtask orchestration
// ---------------------------------------------------------------------------

/// Split a parent task into session tasks for a named team.
///
/// `team_name` must match a `[[teams]]` entry in `.vex/agents.toml`.
/// Returns `Err` with `agents_config_missing` when no config is found,
/// `team_not_found` when the team name does not match, and `Internal` for
/// unexpected I/O errors.
#[tracing::instrument(skip(working_dir, prompt), fields(working_dir = %working_dir.display()))]
pub fn facade_schedule_team(
    working_dir: &Path,
    parent_task_id: &str,
    team_name: &str,
    prompt: &str,
) -> Result<FacadeScheduleTeamResult, ScheduleTeamError> {
    if parent_task_id.trim().is_empty() {
        return Err(ScheduleTeamError::ParentTaskIdRequired);
    }
    if prompt.trim().is_empty() {
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

        let result = Ok(FacadeScheduleTeamResult {
            parent_task_id: decomp.parent_task_id,
            session_task_ids: decomp.session_task_ids,
            scheduler: team_scheduler_name(decomp.scheduler).to_string(),
        });
        if let Err(e) = write_projection_rollup(working_dir) {
            tracing::warn!(error = ?e, "failed to write projection rollup after schedule_team");
        }
        result
    })
}

/// Check the fan-out join gate for a parent task.
///
/// Returns `Ok(None)` when at least one session task is still active.  Returns
/// `Ok(Some(FacadeJoinOutcome))` when every session task has reached a
/// final state.  When the outcome is returned, the caller should call the
/// apply-join endpoint to persist the merged handoff summary.
#[tracing::instrument(skip(working_dir), fields(working_dir = %working_dir.display()))]
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

// ---------------------------------------------------------------------------
// Phase E facade entrypoints — LocalApi session-task projection
// ---------------------------------------------------------------------------

/// Return a summary for every persisted parent-task state file.
///
/// Results are bounded by `StartupBudget::max_scans` to prevent unbounded
/// allocation from large state directories.
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

/// Return a snapshot for every session task across all persisted parent states.
///
/// Bounded by `StartupBudget::max_scans`.
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

/// Return the snapshot for a single session task identified by its UUID.
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

/// Transition a session task to a new lifecycle state reported by an external
/// agent.
///
/// Accepted `status_str` values: `running`, `blocked`, `failed`,
/// `cancelled`, `completed`.
///
/// Transitions from final states (`Failed`, `Cancelled`, `Completed`) are
/// rejected with `TransitionNotAllowed` — use
/// `facade_release_session_task` to clean up a completed task instead.
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

/// Return the full task graph: every parent task with its session tasks.
///
/// Bounded by `StartupBudget::max_scans`.
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

/// Return all active (non-final) session tasks as todo items.
///
/// Bounded by `StartupBudget::max_scans`.
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

// ---------------------------------------------------------------------------
// ADR-046: Peer message channel facade
// ---------------------------------------------------------------------------

use crate::runtime::task_state::peer_channel::{
    self, AppendMessageError, MAX_PEER_MESSAGE_BYTES, PeerMessage, parse_peer_message_kind,
};

/// Post a message to the peer channel for a parent task.
///
/// Validates:
/// 1. The parent task exists in the state directory.
/// 2. The sender is a session task belonging to that parent task.
/// 3. The message content does not exceed `MAX_PEER_MESSAGE_BYTES`.
/// 4. The message kind is a recognised `PeerMessageKind` variant.
#[tracing::instrument(skip_all, fields(parent_task_id = %parent_task_id, sender_id = %sender_id))]
pub fn facade_post_peer_message(
    working_dir: &Path,
    parent_task_id: &str,
    sender_id: &str,
    sender_agent_id: &str,
    recipient: &str,
    kind: &str,
    content: &str,
) -> std::result::Result<PeerMessage, PeerChannelError> {
    // Validate kind
    let kind = parse_peer_message_kind(kind).ok_or(PeerChannelError::InvalidKind)?;

    // Validate content size
    if content.len() > MAX_PEER_MESSAGE_BYTES {
        return Err(PeerChannelError::ContentTooLong);
    }

    let state_dir = TaskState::state_dir_from(working_dir);

    let parent_state = load_parent_task_state(working_dir, parent_task_id)?;
    let sender_task = parent_state
        .session_tasks
        .iter()
        .find(|task| task.id == sender_id)
        .ok_or(PeerChannelError::SenderNotInTask)?;

    if sender_task.agent_id != sender_agent_id {
        tracing::warn!(
            parent_task_id,
            sender_id,
            provided_sender_agent_id = sender_agent_id,
            expected_sender_agent_id = %sender_task.agent_id,
            "peer message sender_agent_id mismatch; using persisted session task agent id"
        );
    }

    let message = PeerMessage::new(
        sender_id,
        sender_task.agent_id.clone(),
        parent_task_id,
        recipient,
        kind,
        content,
    );

    match peer_channel::append_message(&state_dir, &message) {
        Ok(()) => {}
        Err(AppendMessageError::ChannelFull) => return Err(PeerChannelError::ChannelFull),
        Err(AppendMessageError::Internal(err)) => return Err(PeerChannelError::Internal(err)),
    }

    Ok(message)
}

/// Read messages from a parent task's peer channel.
///
/// Returns up to `MAX_CHANNEL_READ_BATCH` messages after the given cursor.
#[tracing::instrument(skip_all, fields(parent_task_id = %parent_task_id, after_ms = after_ms))]
pub fn facade_read_peer_messages(
    working_dir: &Path,
    parent_task_id: &str,
    after_ms: u64,
    recipient_filter: Option<&str>,
) -> Result<Vec<PeerMessage>> {
    let state_dir = TaskState::state_dir_from(working_dir);
    peer_channel::read_messages(&state_dir, parent_task_id, after_ms, recipient_filter)
}

fn load_parent_task_state(
    working_dir: &Path,
    parent_task_id: &str,
) -> std::result::Result<TaskState, PeerChannelError> {
    for dir in TaskState::state_search_dirs_from(working_dir) {
        let state_path = dir.join(format!("{parent_task_id}.json"));
        if !state_path.is_file() {
            continue;
        }

        return TaskState::load(&dir, parent_task_id).map_err(PeerChannelError::Internal);
    }

    Err(PeerChannelError::ParentTaskNotFound)
}
