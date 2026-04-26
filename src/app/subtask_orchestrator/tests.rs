use super::*;
use crate::agents::{AgentProfile, IsolationPolicy, TeamDefinition, TeamScheduler};

fn make_agent(name: &str) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        profile: "default".to_string(),
        isolation: IsolationPolicy::Shared,
        max_parallel_tasks: 2,
        allowed_capabilities: vec![],
    }
}

fn make_team(name: &str, members: &[&str], scheduler: TeamScheduler) -> TeamDefinition {
    TeamDefinition {
        name: name.to_string(),
        members: members.iter().map(|s| s.to_string()).collect(),
        scheduler,
    }
}

fn setup_orchestrator() -> (tempfile::TempDir, SubtaskOrchestrator) {
    let dir = tempfile::tempdir().unwrap();
    let orchestrator = SubtaskOrchestrator::new(dir.path());
    (dir, orchestrator)
}

#[test]
fn fan_out_join_creates_tasks_for_all_team_members() {
    let (_dir, orc) = setup_orchestrator();
    let agents = vec![make_agent("alpha"), make_agent("beta")];
    let team = make_team("pair", &["alpha", "beta"], TeamScheduler::FanOutJoin);
    let decomp = orc
        .schedule_team("parent-1", &team, &agents, "do the work")
        .unwrap();
    assert_eq!(decomp.session_task_ids.len(), 2);
    let state = TaskState::load(orc.state_dir.as_path(), "parent-1").unwrap();
    assert!(state.session_tasks.iter().any(|t| t.agent_id == "alpha"));
    assert!(state.session_tasks.iter().any(|t| t.agent_id == "beta"));
}

#[test]
fn sequential_schedule_creates_only_first_member_task() {
    let (_dir, orc) = setup_orchestrator();
    let agents = vec![make_agent("alpha"), make_agent("beta")];
    let team = make_team("seq-pair", &["alpha", "beta"], TeamScheduler::Sequential);
    let decomp = orc
        .schedule_team("parent-seq", &team, &agents, "do the work")
        .unwrap();
    assert_eq!(decomp.session_task_ids.len(), 1);
    let state = TaskState::load(orc.state_dir.as_path(), "parent-seq").unwrap();
    assert_eq!(
        state
            .session_tasks
            .iter()
            .filter(|t| t.agent_id == "alpha")
            .count(),
        1
    );
    assert_eq!(
        state
            .session_tasks
            .iter()
            .filter(|t| t.agent_id == "beta")
            .count(),
        0
    );
}

#[test]
fn corrupt_parent_state_propagates_error() {
    let (_dir, orc) = setup_orchestrator();
    std::fs::write(
        orc.state_dir.join("parent-corrupt.json"),
        "{ not valid json",
    )
    .unwrap();
    let err = orc
        .schedule_team(
            "parent-corrupt",
            &make_team("t", &["alpha"], TeamScheduler::FanOutJoin),
            &[make_agent("alpha")],
            "work",
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("Failed to deserialize state file"),
        "got: {err:#}"
    );
}
