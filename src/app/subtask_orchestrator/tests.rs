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

fn complete_with_summary(state: &mut TaskState, agent_id: &str, summary: &str) {
    let task = state
        .session_tasks
        .iter_mut()
        .find(|task| task.agent_id == agent_id)
        .expect("session task");
    task.transition_to(SessionTaskStatus::Completed);
    task.set_handoff_summary(summary);
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
    assert!(
        state.session_tasks.iter().all(|t| t.supersedes.is_empty()),
        "independent fan-out members must not declare a replace set"
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

#[test]
fn join_applies_supersession_instead_of_concatenating_summaries() {
    let (_dir, orc) = setup_orchestrator();
    let parent_id = "parent-join-1";
    let parent = TaskState::new(parent_id.to_string());
    parent.save(orc.state_dir.as_path()).unwrap();

    let first = JoinSummary {
        message_id: "child-a".to_string(),
        agent_id: "alpha".to_string(),
        summary: "first child summary".to_string(),
        supersedes: Vec::new(),
    };
    let second = JoinSummary {
        message_id: "child-b".to_string(),
        agent_id: "beta".to_string(),
        summary: "corrected child summary".to_string(),
        supersedes: vec!["child-a".to_string()],
    };
    let outcome = JoinOutcome {
        all_done: true,
        completed: 2,
        failed: 0,
        cancelled: 0,
        summaries: vec![first, second],
    };
    orc.apply_join_outcome(parent_id, &outcome).unwrap();

    let state = TaskState::load(orc.state_dir.as_path(), parent_id).unwrap();
    let handoff = state.handoff_summary.expect("live handoff");
    assert!(
        handoff.contains("corrected child summary"),
        "live CRDT body must appear; got {handoff}"
    );
    assert!(
        !handoff.contains("first child summary"),
        "superseded body must not appear; got {handoff}"
    );
    assert!(
        !handoff.contains("[alpha]: first child summary\n[beta]: corrected child summary"),
        "join must not concatenate every child summary; got {handoff}"
    );

    let record = crate::runtime::WorkingSetRecord::load(orc.state_dir.as_path(), parent_id)
        .expect("peer evidence sidecar");
    assert!(
        record
            .decisions
            .iter()
            .any(|decision| decision.source_reference == "child-b"
                && decision.rationale.contains("corrected child summary")),
        "condenser must record live message id as source_reference; got {:?}",
        record.decisions
    );
    assert!(
        record
            .decisions
            .iter()
            .all(|decision| decision.source_reference != "child-a"),
        "superseded message id must not be recorded; got {:?}",
        record.decisions
    );
}

#[test]
fn poll_fan_out_join_keeps_independent_fan_out_summaries() {
    let (_dir, orc) = setup_orchestrator();
    let agents = vec![make_agent("alpha"), make_agent("beta")];
    let team = make_team("pair", &["alpha", "beta"], TeamScheduler::FanOutJoin);
    orc.schedule_team("parent-fan", &team, &agents, "do the work")
        .unwrap();

    let mut state = TaskState::load(orc.state_dir.as_path(), "parent-fan").unwrap();
    complete_with_summary(&mut state, "alpha", "alpha child summary");
    complete_with_summary(&mut state, "beta", "beta child summary");
    state.save(orc.state_dir.as_path()).unwrap();

    let outcome = orc
        .poll_fan_out_join("parent-fan")
        .unwrap()
        .expect("all session tasks complete");
    assert!(outcome.all_done);
    assert_eq!(outcome.summaries.len(), 2);
    assert!(
        outcome
            .summaries
            .iter()
            .all(|summary| summary.supersedes.is_empty()),
        "independent fan-out members must not replace each other; got {:?}",
        outcome.summaries
    );

    let live = orc.apply_join_outcome("parent-fan", &outcome).unwrap();
    assert_eq!(live.len(), 2);
    let handoff = TaskState::load(orc.state_dir.as_path(), "parent-fan")
        .unwrap()
        .handoff_summary
        .expect("live handoff");
    assert!(handoff.contains("alpha child summary"));
    assert!(handoff.contains("beta child summary"));
}

#[test]
fn poll_fan_out_join_decides_sequential_supersession() {
    let (_dir, orc) = setup_orchestrator();
    let agents = vec![make_agent("alpha"), make_agent("beta")];
    let team = make_team("seq-pair", &["alpha", "beta"], TeamScheduler::Sequential);
    orc.schedule_team("parent-seq-join", &team, &agents, "do the work")
        .unwrap();

    let mut state = TaskState::load(orc.state_dir.as_path(), "parent-seq-join").unwrap();
    complete_with_summary(&mut state, "alpha", "first child summary");
    state.save(orc.state_dir.as_path()).unwrap();

    orc.advance_sequential("parent-seq-join", &team, &agents, "do the work")
        .unwrap()
        .expect("beta scheduled after alpha completed");

    let mut state = TaskState::load(orc.state_dir.as_path(), "parent-seq-join").unwrap();
    let alpha_id = state
        .session_tasks
        .iter()
        .find(|task| task.agent_id == "alpha")
        .map(|task| task.id.clone())
        .expect("alpha");
    let beta = state
        .session_tasks
        .iter()
        .find(|task| task.agent_id == "beta")
        .expect("beta");
    assert_eq!(
        beta.supersedes,
        vec![alpha_id.clone()],
        "sequential continuation must declare the prior member"
    );
    complete_with_summary(&mut state, "beta", "corrected child summary");
    state.save(orc.state_dir.as_path()).unwrap();

    let outcome = orc
        .poll_fan_out_join("parent-seq-join")
        .unwrap()
        .expect("all session tasks complete");
    let beta_summary = outcome
        .summaries
        .iter()
        .find(|summary| summary.agent_id == "beta")
        .expect("beta summary");
    assert_eq!(beta_summary.supersedes, vec![alpha_id.clone()]);

    let live = orc.apply_join_outcome("parent-seq-join", &outcome).unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].body, "corrected child summary");
    let handoff = TaskState::load(orc.state_dir.as_path(), "parent-seq-join")
        .unwrap()
        .handoff_summary
        .expect("live handoff");
    assert!(handoff.contains("corrected child summary"));
    assert!(
        !handoff.contains("first child summary"),
        "sequential join must not concatenate prior member summaries; got {handoff}"
    );
}

#[test]
fn poll_fan_out_join_same_agent_later_completion_replaces_earlier() {
    let (_dir, orc) = setup_orchestrator();
    let parent_id = "parent-retry";
    let mut parent = TaskState::new(parent_id.to_string());
    let mut first = SessionTask::new(parent_id, "alpha", "first attempt", None);
    first.transition_to(SessionTaskStatus::Completed);
    first.set_handoff_summary("first child summary");
    let first_id = first.id.clone();
    let mut retry = SessionTask::new(parent_id, "alpha", "retry", None);
    retry.transition_to(SessionTaskStatus::Completed);
    retry.set_handoff_summary("retry child summary");
    parent.add_session_task(first);
    parent.add_session_task(retry);
    parent.save(orc.state_dir.as_path()).unwrap();

    let outcome = orc
        .poll_fan_out_join(parent_id)
        .unwrap()
        .expect("all session tasks complete");
    let retry_summary = outcome
        .summaries
        .iter()
        .find(|summary| summary.summary == "retry child summary")
        .expect("retry summary");
    assert_eq!(retry_summary.supersedes, vec![first_id]);

    let live = orc.apply_join_outcome(parent_id, &outcome).unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].body, "retry child summary");
}
