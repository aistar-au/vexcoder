use thiserror::Error;

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

/// Thin domain structs the transport layer maps to JSON.
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

/// One node in the task graph: a parent task plus its session tasks.
#[derive(Debug, Clone)]
pub struct FacadeTaskGraphNode {
    pub id: String,
    pub status: String,
    pub agent_id: Option<String>,
    pub session_tasks: Vec<FacadeSessionTaskSnapshot>,
}

/// Top-level task graph returned by `facade_task_graph`.
#[derive(Debug, Clone)]
pub struct FacadeTaskGraph {
    pub nodes: Vec<FacadeTaskGraphNode>,
}

/// One live (non-terminal) session task returned by `facade_list_todos`.
#[derive(Debug, Clone)]
pub struct FacadeTodoItem {
    pub id: String,
    pub parent_task_id: String,
    pub agent_id: String,
    pub lifecycle_state: String,
}
