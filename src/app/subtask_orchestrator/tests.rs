use super::*;
use crate::agents::{AgentProfile, IsolationPolicy, TeamDefinition, TeamScheduler};
use crate::runtime::SessionTaskStatus;

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
fn fan_out_join_creates_tasks_for_all_members() {
    let (_dir, orc) = setup_orchestrator();
    let agents = vec![make_agent("alpha"), make_agent("beta")];
    let team = make_team("pair", &["alpha", "beta"], TeamScheduler::FanOutJoin);

    let decomp = orc
        .schedule_team("parent-1", &team, &agents, "do the work")
        .unwrap();

    assert_eq!(decomp.scheduler, TeamScheduler::FanOutJoin);
    assert_eq!(decomp.session_task_ids.len(), 2);

    let state = TaskState::load(orc.state_dir.as_path(), "parent-1").unwrap();
    assert_eq!(state.session_tasks.len(), 2);
    let agent_ids: Vec<_> = state.session_tasks.iter().map(|t| &t.agent_id).collect();
    assert!(agent_ids.iter().any(|id| *id == "alpha"));
    assert!(agent_ids.iter().any(|id| *id == "beta"));
}

#[test]
fn schedule_team_propagates_corrupt_parent_state_errors() {
    let (_dir, orc) = setup_orchestrator();
    let agents = vec![make_agent("alpha")];
    let team = make_team("single", &["alpha"], TeamScheduler::FanOutJoin);

    std::fs::write(
        orc.state_dir.join("parent-corrupt.json"),
        "{ not valid json",
    )
    .unwrap();

    let error = orc
        .schedule_team("parent-corrupt", &team, &agents, "do the work")
        .expect_err("corrupt parent state should fail");

    assert!(
        error
            .to_string()
            .contains("Failed to deserialize state file"),
        "expected deserialize error, got: {error:#}"
    );
}

#[test]
fn sequential_schedule_creates_only_first_member_task() {
    let (_dir, orc) = setup_orchestrator();
    let agents = vec![make_agent("alpha"), make_agent("beta")];
    let team = make_team("seq-pair", &["alpha", "beta"], TeamScheduler::Sequential);

    let decomp = orc
        .schedule_team("parent-seq", &team, &agents, "do the work")
        .unwrap();

    assert_eq!(decomp.scheduler, TeamScheduler::Sequential);
    assert_eq!(decomp.session_task_ids.len(), 1);

    let state = TaskState::load(orc.state_dir.as_path(), "parent-seq").unwrap();
    assert_eq!(state.session_tasks.len(), 1);
    assert_eq!(state.session_tasks[0].agent_id, "alpha");
}

#[test]
fn poll_fan_out_join_returns_none_when_tasks_are_live() {
    let (_dir, orc) = setup_orchestrator();
    let agents = vec![make_agent("alpha"), make_agent("beta")];
    let team = make_team("pair", &["alpha", "beta"], TeamScheduler::FanOutJoin);

    orc.schedule_team("parent-poll", &team, &agents, "work")
        .unwrap();

    // Tasks default to Pending (active) — join should not fire.
    let outcome = orc.poll_fan_out_join("parent-poll").unwrap();
    assert!(outcome.is_none(), "expected None while tasks are active");
}

#[test]
fn poll_fan_out_join_returns_outcome_when_all_terminal() {
    let (_dir, orc) = setup_orchestrator();
    let agents = vec![make_agent("alpha"), make_agent("beta")];
    let team = make_team("pair", &["alpha", "beta"], TeamScheduler::FanOutJoin);

    orc.schedule_team("parent-done", &team, &agents, "work")
        .unwrap();

    // Transition both tasks to final states.
    let mut state = TaskState::load(orc.state_dir.as_path(), "parent-done").unwrap();
    let ids: Vec<String> = state.session_tasks.iter().map(|t| t.id.clone()).collect();
    for id in &ids {
        state.update_session_task_status(id, SessionTaskStatus::Completed);
    }
    state.session_tasks[0].handoff_summary = Some("alpha done".to_string());
    state.save(orc.state_dir.as_path()).unwrap();

    let outcome = orc.poll_fan_out_join("parent-done").unwrap().unwrap();
    assert!(outcome.all_done);
    assert_eq!(outcome.completed, 2);
    assert_eq!(outcome.failed, 0);
    assert_eq!(outcome.summaries.len(), 1);
    assert_eq!(outcome.summaries[0].0, "alpha");
    assert_eq!(outcome.summaries[0].1, "alpha done");
}

#[test]
fn advance_sequential_returns_none_while_first_task_is_live() {
    let (_dir, orc) = setup_orchestrator();
    let agents = vec![make_agent("alpha"), make_agent("beta")];
    let team = make_team("seq", &["alpha", "beta"], TeamScheduler::Sequential);

    // Create only alpha's task (sequential first step).
    orc.schedule_team("parent-seq2", &team, &agents, "work")
        .unwrap();

    // alpha is still Pending — advance should block.
    let next = orc
        .advance_sequential("parent-seq2", &team, &agents, "work")
        .unwrap();
    assert!(next.is_none(), "expected None while alpha is still active");
}

#[test]
fn advance_sequential_creates_next_task_after_first_completes() {
    let (_dir, orc) = setup_orchestrator();
    let agents = vec![make_agent("alpha"), make_agent("beta")];
    let team = make_team("seq", &["alpha", "beta"], TeamScheduler::Sequential);

    orc.schedule_team("parent-seq3", &team, &agents, "work")
        .unwrap();

    // Transition alpha to Completed.
    let mut state = TaskState::load(orc.state_dir.as_path(), "parent-seq3").unwrap();
    let alpha_id = state.session_tasks[0].id.clone();
    state.update_session_task_status(&alpha_id, SessionTaskStatus::Completed);
    state.save(orc.state_dir.as_path()).unwrap();

    // Now advance should create beta's task.
    let next = orc
        .advance_sequential("parent-seq3", &team, &agents, "work")
        .unwrap();
    assert!(next.is_some(), "expected a new task ID for beta");

    let state = TaskState::load(orc.state_dir.as_path(), "parent-seq3").unwrap();
    assert_eq!(state.session_tasks.len(), 2);
    let beta_task = state.session_tasks.iter().find(|t| t.agent_id == "beta");
    assert!(beta_task.is_some(), "beta task should now exist");
}

#[test]
fn advance_sequential_returns_none_when_all_members_done() {
    let (_dir, orc) = setup_orchestrator();
    let agents = vec![make_agent("alpha"), make_agent("beta")];
    let team = make_team("seq", &["alpha", "beta"], TeamScheduler::Sequential);

    orc.schedule_team("parent-seq4", &team, &agents, "work")
        .unwrap();

    // Mark alpha done, advance to create beta.
    let mut state = TaskState::load(orc.state_dir.as_path(), "parent-seq4").unwrap();
    let alpha_id = state.session_tasks[0].id.clone();
    state.update_session_task_status(&alpha_id, SessionTaskStatus::Completed);
    state.save(orc.state_dir.as_path()).unwrap();

    orc.advance_sequential("parent-seq4", &team, &agents, "work")
        .unwrap();

    // Mark beta done too.
    let mut state = TaskState::load(orc.state_dir.as_path(), "parent-seq4").unwrap();
    let beta_id = state
        .session_tasks
        .iter()
        .find(|t| t.agent_id == "beta")
        .unwrap()
        .id
        .clone();
    state.update_session_task_status(&beta_id, SessionTaskStatus::Completed);
    state.save(orc.state_dir.as_path()).unwrap();

    // All done — advance should return None.
    let next = orc
        .advance_sequential("parent-seq4", &team, &agents, "work")
        .unwrap();
    assert!(next.is_none(), "expected None after all members completed");
}

#[test]
fn apply_join_outcome_sets_parent_handoff_summary() {
    let (_dir, orc) = setup_orchestrator();
    let state = TaskState::new("parent-join".to_string());
    state.save(orc.state_dir.as_path()).unwrap();

    let outcome = JoinOutcome {
        all_done: true,
        completed: 2,
        failed: 0,
        cancelled: 0,
        summaries: vec![
            ("alpha".to_string(), "alpha result".to_string()),
            ("beta".to_string(), "beta result".to_string()),
        ],
    };

    orc.apply_join_outcome("parent-join", &outcome).unwrap();

    let state = TaskState::load(orc.state_dir.as_path(), "parent-join").unwrap();
    let summary = state.handoff_summary.unwrap();
    assert!(summary.contains("[alpha]: alpha result"));
    assert!(summary.contains("[beta]: beta result"));
}

#[test]
fn is_team_schedule_exhausted_returns_false_while_tasks_are_live() {
    let (_dir, orc) = setup_orchestrator();
    let agents = vec![make_agent("alpha"), make_agent("beta")];
    let team = make_team("pair", &["alpha", "beta"], TeamScheduler::FanOutJoin);

    orc.schedule_team("parent-ex", &team, &agents, "work")
        .unwrap();

    assert!(
        !orc.is_team_schedule_exhausted("parent-ex", &team).unwrap(),
        "should not be exhausted while tasks are active"
    );
}

#[test]
fn is_team_schedule_exhausted_returns_true_when_all_done() {
    let (_dir, orc) = setup_orchestrator();
    let agents = vec![make_agent("alpha"), make_agent("beta")];
    let team = make_team("pair", &["alpha", "beta"], TeamScheduler::FanOutJoin);

    orc.schedule_team("parent-ex2", &team, &agents, "work")
        .unwrap();

    let mut state = TaskState::load(orc.state_dir.as_path(), "parent-ex2").unwrap();
    let ids: Vec<String> = state.session_tasks.iter().map(|t| t.id.clone()).collect();
    for id in &ids {
        state.update_session_task_status(id, SessionTaskStatus::Completed);
    }
    state.save(orc.state_dir.as_path()).unwrap();

    assert!(
        orc.is_team_schedule_exhausted("parent-ex2", &team).unwrap(),
        "should be exhausted after all tasks complete"
    );
}

#[test]
fn live_session_task_count_returns_correct_count() {
    let (_dir, orc) = setup_orchestrator();
    let agents = vec![make_agent("alpha"), make_agent("beta")];
    let team = make_team("pair", &["alpha", "beta"], TeamScheduler::FanOutJoin);

    orc.schedule_team("parent-count", &team, &agents, "work")
        .unwrap();

    // Both tasks are Pending (active).
    assert_eq!(orc.live_session_task_count("parent-count").unwrap(), 2);

    // Complete one.
    let mut state = TaskState::load(orc.state_dir.as_path(), "parent-count").unwrap();
    let first_id = state.session_tasks[0].id.clone();
    state.update_session_task_status(&first_id, SessionTaskStatus::Completed);
    state.save(orc.state_dir.as_path()).unwrap();

    assert_eq!(orc.live_session_task_count("parent-count").unwrap(), 1);
}
