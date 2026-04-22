

use anyhow::{Result, anyhow, bail};
use std::path::PathBuf;

use crate::agents::{AgentProfile, IsolationPolicy, TeamDefinition, TeamScheduler};
use crate::runtime::{SessionTask, SessionTaskStatus, TaskState, WorktreeLeaseManager};


#[derive(Debug, Clone)]
pub struct TeamDecomposition {
    
    pub parent_task_id: String,
    
    
    pub session_task_ids: Vec<String>,
    
    pub scheduler: TeamScheduler,
}


#[derive(Debug, Clone)]
pub struct JoinOutcome {
    
    pub all_done: bool,
    
    pub completed: usize,
    
    pub failed: usize,
    
    pub cancelled: usize,
    
    
    pub summaries: Vec<(String, String)>,
}


#[derive(Debug, Clone)]
pub struct SubtaskOrchestrator {
    state_dir: PathBuf,
}

impl SubtaskOrchestrator {
    
    pub fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
        }
    }

    
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

    
    #[tracing::instrument(skip(self))]
    pub fn poll_fan_out_join(&self, parent_task_id: &str) -> Result<Option<JoinOutcome>> {
        let state = TaskState::load(&self.state_dir, parent_task_id)?;

        if state.session_tasks.is_empty() {
            
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

    
    #[tracing::instrument(skip(self, team, agents, prompt))]
    pub fn advance_sequential(
        &self,
        parent_task_id: &str,
        team: &TeamDefinition,
        agents: &[AgentProfile],
        prompt: &str,
    ) -> Result<Option<String>> {
        let mut parent_state = TaskState::load(&self.state_dir, parent_task_id)?;

        
        let mut next_member: Option<&str> = None;

        for member_name in &team.members {
            let existing = parent_state
                .session_tasks
                .iter()
                .find(|t| &t.agent_id == member_name);

            match existing {
                None => {
                    
                    next_member = Some(member_name.as_str());
                    break;
                }
                Some(task) if task.lifecycle_state.is_live() => {
                    
                    return Ok(None);
                }
                Some(_) => {
                    
                }
            }
        }

        let member_name = match next_member {
            Some(name) => name,
            None => return Ok(None), 
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

    
    #[tracing::instrument(skip(self))]
    pub fn live_session_task_count(&self, parent_task_id: &str) -> Result<usize> {
        let state = TaskState::load(&self.state_dir, parent_task_id)?;
        Ok(state
            .session_tasks
            .iter()
            .filter(|t| t.lifecycle_state.is_live())
            .count())
    }

    
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


fn find_agent<'a>(agents: &'a [AgentProfile], name: &str) -> Result<&'a AgentProfile> {
    agents
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| anyhow!("agent '{}' not found in provided agent list", name))
}


#[cfg(test)]
mod tests;
