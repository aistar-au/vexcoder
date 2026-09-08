use anyhow::Result;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use thiserror::Error;

use crate::agents::{IsolationPolicy, TeamScheduler, load_agents_config};
use crate::app::subtask_orchestrator::SubtaskOrchestrator;
use crate::runtime::{
    SessionTask, SessionTaskStatus, StateEnvelope, TaskState, WorktreeLeaseManager,
};

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

#[derive(Debug, Error)]
pub enum DelegateError {
    #[error("agent_not_found")]
    AgentNotFound,
    #[error("agents_config_missing")]
    AgentsConfigMissing,
    #[error("parent_task_id_required")]
    ParentTaskIdRequired,

    #[error("concurrency_limit_reached")]
    ConcurrencyLimitReached,

    #[error("prompt_too_long")]
    PromptTooLong,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}
