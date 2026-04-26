use std::io::Write;
use std::path::Path;
use vexcoder::agents::{IsolationPolicy, TeamScheduler, load_agents_config};

fn write_agents_toml(root: &Path, content: &str) {
    let vex_dir = root.join(".vex");
    std::fs::create_dir_all(&vex_dir).unwrap();
    let mut f = std::fs::File::create(vex_dir.join("agents.toml")).unwrap();
    f.write_all(content.as_bytes()).unwrap();
}

#[test]
fn loads_agents_and_teams_and_walks_ancestor_directories() {
    let dir = tempfile::tempdir().unwrap();
    write_agents_toml(
        dir.path(),
        r#"
[[agents]]
name = "coder"
isolation = "worktree"
max_parallel_tasks = 2
allowed_capabilities = ["read-file", "apply-patch"]

[[agents]]
name = "reviewer"
isolation = "shared"

[[teams]]
name = "dev-team"
members = ["coder", "reviewer"]
scheduler = "fan_out_join"

[[teams]]
name = "ordered"
members = ["coder", "reviewer"]
scheduler = "sequential"
"#,
    );

    let config = load_agents_config(dir.path()).unwrap().unwrap();
    assert_eq!(config.agent_profiles[0].name, "coder");
    assert_eq!(
        config.agent_profiles[0].isolation,
        IsolationPolicy::Worktree
    );
    assert_eq!(config.agent_profiles[0].max_parallel_tasks, 2);
    assert_eq!(config.agent_profiles[1].isolation, IsolationPolicy::Shared);
    assert_eq!(
        config.team_definitions[0].scheduler,
        TeamScheduler::FanOutJoin
    );
    assert_eq!(
        config.team_definitions[1].scheduler,
        TeamScheduler::Sequential
    );
    assert!(
        load_agents_config(tempfile::tempdir().unwrap().path())
            .unwrap()
            .is_none(),
        "no config returns None"
    );

    // ancestor walk
    let nested = dir.path().join("a/b/c");
    std::fs::create_dir_all(&nested).unwrap();
    assert_eq!(
        load_agents_config(&nested).unwrap().unwrap().agent_profiles[0].name,
        "coder"
    );
}
