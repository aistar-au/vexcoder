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
