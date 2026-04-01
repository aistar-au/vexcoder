//! ADR-034 Phase C — Subtask orchestration.
//!
//! The `SubtaskOrchestrator` is the sole authority for decomposing a parent
//! task into session tasks, driving scheduler state transitions, and merging
//! join results back into parent task state.  It implements the two scheduler
//! strategies declared in `AgentsConfig`:
//!
//! - `FanOutJoin` — all member session tasks are created immediately; the join
//!   gate resolves once every task reaches a final state.
//! - `Sequential` — only the first member's task is created; subsequent tasks
//!   are created one at a time after the preceding task completes.
//!
//! The orchestrator operates directly on the persisted task-state directory
//! and does not hold in-memory state across calls, so it is safe to
//! instantiate per request and discard.

use anyhow::{anyhow, bail, Result};
use std::path::PathBuf;

use crate::agents::{AgentProfile, IsolationPolicy, TeamDefinition, TeamScheduler};
use crate::runtime::{SessionTask, SessionTaskStatus, TaskState, WorktreeLeaseManager};

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// Records the session tasks created by a `schedule_team` call.
#[derive(Debug, Clone)]
pub struct TeamDecomposition {
    /// Parent task the session tasks were attached to.
    pub parent_task_id: String,
    /// IDs of the session tasks that were created in this call.
    ///
    /// For `FanOutJoin` this is all members; for `Sequential` this is the
    /// first member only.
    pub session_task_ids: Vec<String>,
    /// Scheduler that governed task creation.
    pub scheduler: TeamScheduler,
}

/// Records the outcome of a completed join poll.
#[derive(Debug, Clone)]
pub struct JoinOutcome {
    /// `true` when every session task has reached a final state.
    pub all_done: bool,
    /// Number of tasks that completed successfully.
    pub completed: usize,
    /// Number of tasks that ended in the `Failed` state.
    pub failed: usize,
    /// Number of tasks that ended in the `Cancelled` state.
    pub cancelled: usize,
    /// Handoff summaries from completed tasks: `(agent_id, summary)`.
    ///
    /// Only tasks that recorded a `handoff_summary` contribute an entry.
    pub summaries: Vec<(String, String)>,
}

// ---------------------------------------------------------------------------
// SubtaskOrchestrator
// ---------------------------------------------------------------------------

/// Drives ADR-034 Phase C subtask lifecycle from the persisted task-state
/// directory.
#[derive(Debug, Clone)]
pub struct SubtaskOrchestrator {
    state_dir: PathBuf,
}

impl SubtaskOrchestrator {
    /// Create an orchestrator bound to `state_dir`.
    pub fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
        }
    }

    // -----------------------------------------------------------------------
    // Extraction
    // -----------------------------------------------------------------------

    /// Extract a parent task into session tasks following the team scheduler.
    ///
    /// `FanOutJoin`: session tasks are created for every team member in
    /// declaration order.
    ///
    /// `Sequential`: a session task is created only for the first member.
    /// Call [`advance_sequential`] after the current task completes to create
    /// the next.
    ///
    /// The parent task state file is created (via `TaskState::new`) when it
    /// does not already exist.
    #[tracing::instrument(skip(self, team, agents, prompt))]
    pub fn schedule_team(
        &self,
        parent_task_id: &str,
        team: &TeamDefinition,
        agents: &[AgentProfile],
        prompt: &str,
    ) -> Result<TeamDecomposition> {
        if team.members.is_empty() {
            bail!("team '{}' has no members", team.name);
        }

        let parent_state_path = self.state_dir.join(format!("{parent_task_id}.json"));
        let mut parent_state = if parent_state_path.exists() {
            TaskState::load(&self.state_dir, parent_task_id)?
        } else {
            TaskState::new(parent_task_id.to_string())
        };

        let members_to_create: &[String] = match team.scheduler {
            TeamScheduler::FanOutJoin => &team.members,
            TeamScheduler::Sequential => &team.members[..1],
        };

        let lease_manager = WorktreeLeaseManager::new(&self.state_dir);
        let mut session_task_ids = Vec::with_capacity(members_to_create.len());

        for member_name in members_to_create {
            let agent = find_agent(agents, member_name)?;
            let mut session_task =
                SessionTask::new(parent_task_id, member_name.as_str(), prompt, None);
            let task_id = session_task.id.clone();

            if agent.isolation == IsolationPolicy::Worktree {
                let lease = lease_manager.lease_for_task(&task_id, Some(parent_task_id))?;
                session_task.worktree_path = Some(lease.path);
            }

            session_task_ids.push(task_id);
            parent_state.add_session_task(session_task);
        }

        std::fs::create_dir_all(&self.state_dir)?;
        parent_state.save(&self.state_dir)?;

        Ok(TeamDecomposition {
            parent_task_id: parent_task_id.to_string(),
            session_task_ids,
            scheduler: team.scheduler,
        })
    }

    // -----------------------------------------------------------------------
    // FanOutJoin gate
    // -----------------------------------------------------------------------

    /// Check whether all session tasks attached to `parent_task_id` are done.
    ///
    /// Returns `None` when at least one task is still active (`Pending`,
    /// `Running`, or `Blocked`).  Returns `Some(JoinOutcome)` when every
    /// task has reached a final state (`Completed`, `Failed`, or
    /// `Cancelled`).
    ///
    /// The caller is responsible for calling [`apply_join_outcome`] once the
    /// outcome is available.
    #[tracing::instrument(skip(self))]
    pub fn poll_fan_out_join(&self, parent_task_id: &str) -> Result<Option<JoinOutcome>> {
        let state = TaskState::load(&self.state_dir, parent_task_id)?;

        if state.session_tasks.is_empty() {
            // No session tasks exist yet — nothing to join.
            return Ok(None);
        }

        let mut completed = 0usize;
        let mut failed = 0usize;
        let mut cancelled = 0usize;
        let mut summaries = Vec::new();

        for task in &state.session_tasks {
            match &task.lifecycle_state {
                SessionTaskStatus::Completed => {
                    completed += 1;
                    if let Some(summary) = &task.handoff_summary {
                        summaries.push((task.agent_id.clone(), summary.clone()));
                    }
                }
                SessionTaskStatus::Failed => {
                    failed += 1;
                }
                SessionTaskStatus::Cancelled => {
                    cancelled += 1;
                }
                _ => {
                    // Still active — join gate is not satisfied.
                    return Ok(None);
                }
            }
        }

        Ok(Some(JoinOutcome {
            all_done: true,
            completed,
            failed,
            cancelled,
            summaries,
        }))
    }

    // -----------------------------------------------------------------------
    // Sequential advance
    // -----------------------------------------------------------------------

    /// Advance a sequential schedule by creating the next member's session task.
    ///
    /// Walks `team.members` in declaration order to find the first member
    /// that has no session task yet.  If the preceding task is still active
    /// the function returns `Ok(None)` — the caller must wait.  If all
    /// members have completed tasks the function returns `Ok(None)` to signal
    /// schedule exhaustion.
    ///
    /// Returns `Ok(Some(session_task_id))` when a new task was created and
    /// persisted.
    #[tracing::instrument(skip(self, team, agents, prompt))]
    pub fn advance_sequential(
        &self,
        parent_task_id: &str,
        team: &TeamDefinition,
        agents: &[AgentProfile],
        prompt: &str,
    ) -> Result<Option<String>> {
        let mut parent_state = TaskState::load(&self.state_dir, parent_task_id)?;

        // Find the next member to schedule:
        // - skip members whose task has reached a final state,
        // - return None when the current member's task is still active,
        // - create a task for the first member with no task entry.
        let mut next_member: Option<&str> = None;

        for member_name in &team.members {
            let existing = parent_state
                .session_tasks
                .iter()
                .find(|t| &t.agent_id == member_name);

            match existing {
                None => {
                    // Member has no task — it is the next to create.
                    next_member = Some(member_name.as_str());
                    break;
                }
                Some(task) if task.lifecycle_state.is_live() => {
                    // Current task is still running — cannot advance yet.
                    return Ok(None);
                }
                Some(_) => {
                    // Final state — continue to the next member.
                }
            }
        }

        let member_name = match next_member {
            Some(name) => name,
            None => return Ok(None), // All members have completed tasks.
        };

        let agent = find_agent(agents, member_name)?;
        let lease_manager = WorktreeLeaseManager::new(&self.state_dir);
        let mut session_task = SessionTask::new(parent_task_id, member_name, prompt, None);
        let task_id = session_task.id.clone();

        if agent.isolation == IsolationPolicy::Worktree {
            let lease = lease_manager.lease_for_task(&task_id, Some(parent_task_id))?;
            session_task.worktree_path = Some(lease.path);
        }

        parent_state.add_session_task(session_task);
        parent_state.save(&self.state_dir)?;

        Ok(Some(task_id))
    }

    // -----------------------------------------------------------------------
    // Join result merge
    // -----------------------------------------------------------------------

    /// Merge `JoinOutcome` summaries into the parent task's `handoff_summary`.
    ///
    /// Each completed session task that recorded a `handoff_summary` contributes
    /// one entry of the form `[agent_id]: summary`, joined with newlines.
    ///
    /// When `outcome.summaries` is empty the function is a no-op.
    #[tracing::instrument(skip(self, outcome))]
    pub fn apply_join_outcome(&self, parent_task_id: &str, outcome: &JoinOutcome) -> Result<()> {
        if outcome.summaries.is_empty() {
            return Ok(());
        }
        let mut state = TaskState::load(&self.state_dir, parent_task_id)?;
        let merged = outcome
            .summaries
            .iter()
            .map(|(agent_id, summary)| format!("[{agent_id}]: {summary}"))
            .collect::<Vec<_>>()
            .join("\n");
        state.handoff_summary = Some(merged);
        state.touch();
        state.save(&self.state_dir)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Graph query helpers
    // -----------------------------------------------------------------------

    /// Return the number of active session tasks attached to `parent_task_id`.
    ///
    /// A task is active when its `lifecycle_state` is `Pending`, `Running`, or
    /// `Blocked`.
    #[tracing::instrument(skip(self))]
    pub fn live_session_task_count(&self, parent_task_id: &str) -> Result<usize> {
        let state = TaskState::load(&self.state_dir, parent_task_id)?;
        Ok(state
            .session_tasks
            .iter()
            .filter(|t| t.lifecycle_state.is_live())
            .count())
    }

    /// Return `true` when every member of `team` has a session task in a
    /// final state attached to `parent_task_id`.
    #[tracing::instrument(skip(self, team))]
    pub fn is_team_schedule_exhausted(
        &self,
        parent_task_id: &str,
        team: &TeamDefinition,
    ) -> Result<bool> {
        let state = TaskState::load(&self.state_dir, parent_task_id)?;
        let exhausted = team.members.iter().all(|member_name| {
            state
                .session_tasks
                .iter()
                .any(|t| &t.agent_id == member_name && !t.lifecycle_state.is_live())
        });
        Ok(exhausted)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn find_agent<'a>(agents: &'a [AgentProfile], name: &str) -> Result<&'a AgentProfile> {
    agents
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| anyhow!("agent '{}' not found in provided agent list", name))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
