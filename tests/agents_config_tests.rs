use std::io::Write;
use std::path::Path;
use vexcoder::agents::{IsolationPolicy, TeamScheduler, load_agents_config};

fn write_agents_toml(root: &Path, content: &str) {
    let vex_dir = root.join(".vex");
    std::fs::create_dir_all(&vex_dir).unwrap();
    let path = vex_dir.join("agents.toml");
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(content.as_bytes()).unwrap();
}

#[test]
fn loads_agents_from_workspace_root() {
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
allowed_capabilities = ["read-file"]

[[teams]]
name = "dev-team"
members = ["coder", "reviewer"]
scheduler = "fan_out_join"
"#,
    );

    let config = load_agents_config(dir.path()).unwrap().unwrap();

    assert_eq!(config.agent_profiles.len(), 2);
    let coder = &config.agent_profiles[0];
    assert_eq!(coder.name, "coder");
    assert_eq!(coder.isolation, IsolationPolicy::Worktree);
    assert_eq!(coder.max_parallel_tasks, 2);
    assert_eq!(coder.allowed_capabilities, vec!["read-file", "apply-patch"]);

    let reviewer = &config.agent_profiles[1];
    assert_eq!(reviewer.name, "reviewer");
    assert_eq!(reviewer.isolation, IsolationPolicy::Shared);
    assert_eq!(reviewer.max_parallel_tasks, 1);

    assert_eq!(config.team_definitions.len(), 1);
    let team = &config.team_definitions[0];
    assert_eq!(team.name, "dev-team");
    assert_eq!(team.scheduler, TeamScheduler::FanOutJoin);
}

#[test]
fn returns_none_without_config_file() {
    let dir = tempfile::tempdir().unwrap();
    assert!(load_agents_config(dir.path()).unwrap().is_none());
}

#[test]
fn walks_ancestors_to_find_config() {
    let dir = tempfile::tempdir().unwrap();
    write_agents_toml(dir.path(), "[[agents]]\nname = \"deep\"\n");

    let nested = dir.path().join("a").join("b").join("c");
    std::fs::create_dir_all(&nested).unwrap();

    let config = load_agents_config(&nested).unwrap().unwrap();
    assert_eq!(config.agent_profiles[0].name, "deep");
}

#[test]
fn sequential_team_scheduler() {
    let dir = tempfile::tempdir().unwrap();
    write_agents_toml(
        dir.path(),
        r#"
[[agents]]
name = "a"
[[agents]]
name = "b"
[[teams]]
name = "ordered"
members = ["a", "b"]
scheduler = "sequential"
"#,
    );
    let config = load_agents_config(dir.path()).unwrap().unwrap();
    assert_eq!(
        config.team_definitions[0].scheduler,
        TeamScheduler::Sequential
    );
}
