use anyhow::Result;
use std::path::Path;

use crate::agents::load_agents_config;
use crate::app::subtask_orchestrator::SubtaskOrchestrator;
use crate::runtime::TaskState;

use super::projection::write_projection_rollup;
use super::types::{FacadeJoinOutcome, FacadeScheduleTeamResult, ScheduleTeamError};
use super::{
    run_delegate_race_hook, team_scheduler_name, with_delegate_lock, MAX_DELEGATE_PROMPT_BYTES,
};

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

#[tracing::instrument(skip(working_dir), fields(working_dir = %working_dir.display()))]
pub fn facade_poll_join(
    working_dir: &Path,
    parent_task_id: &str,
) -> Result<Option<FacadeJoinOutcome>> {
    let state_dir = TaskState::state_dir_from(working_dir);
    let orchestrator = SubtaskOrchestrator::new(&state_dir);
    let Some(outcome) = orchestrator.poll_fan_out_join(parent_task_id)? else {
        return Ok(None);
    };
    let live = if outcome.all_done {
        orchestrator.apply_join_outcome(parent_task_id, &outcome)?
    } else {
        Vec::new()
    };
    Ok(Some(FacadeJoinOutcome {
        all_done: outcome.all_done,
        completed: outcome.completed,
        failed: outcome.failed,
        cancelled: outcome.cancelled,
        summaries: live
            .into_iter()
            .map(|entry| (entry.agent_id, entry.body))
            .collect(),
    }))
}
