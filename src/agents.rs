//! Agent definitions surface for `.vex/agents.toml`.
//!
//! ADR-034 Phase A: parsing, validation, team composition rules, and
//! repo-local discovery.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ── Types ──────────────────────────────────────────────────────────

/// Top-level agents configuration file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentsConfig {
    #[serde(default, rename = "agents")]
    pub agent_profiles: Vec<AgentProfile>,
    #[serde(default, rename = "teams")]
    pub team_definitions: Vec<TeamDefinition>,
}

/// A single named agent profile.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProfile {
    pub name: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default)]
    pub isolation: IsolationPolicy,
    #[serde(default = "default_max_parallel")]
    pub max_parallel_tasks: u32,
    #[serde(default)]
    pub allowed_capabilities: Vec<String>,
}

/// Isolation policy per ADR-034 §3.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IsolationPolicy {
    /// Dedicated git worktree — required for concurrent mutable agents.
    #[default]
    Worktree,
    /// Share parent worktree — only permitted for read-only tasks.
    Shared,
}

/// Scheduler strategy for team execution.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TeamScheduler {
    /// Fan out to all members, then join results.
    #[default]
    FanOutJoin,
    /// Execute members sequentially in order.
    Sequential,
}

/// Named team composed from agent profiles.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamDefinition {
    pub name: String,
    pub members: Vec<String>,
    #[serde(default)]
    pub scheduler: TeamScheduler,
}

// ── Defaults ───────────────────────────────────────────────────────

fn default_profile() -> String {
    "default".to_string()
}

fn default_max_parallel() -> u32 {
    1
}

// ── Loading ────────────────────────────────────────────────────────

/// Locate `.vex/agents.toml` by walking ancestors from `cwd`.
pub fn find_agents_config(cwd: &Path) -> Option<PathBuf> {
    let mut dir: &Path = cwd;
    loop {
        let candidate = dir.join(".vex").join("agents.toml");
        if candidate.exists() {
            return Some(candidate);
        }
        dir = dir.parent()?;
    }
}

/// Load and validate the agents configuration.
///
/// Returns `Ok(None)` when no `.vex/agents.toml` is found (the single-agent
/// runtime path remains the default).
pub fn load_agents_config(cwd: &Path) -> Result<Option<AgentsConfig>> {
    let path = match find_agents_config(cwd) {
        Some(p) => p,
        None => return Ok(None),
    };
    load_agents_config_from_path(&path)
}

/// Load from an explicit path (used by tests and `load_agents_config`).
pub fn load_agents_config_from_path(path: &Path) -> Result<Option<AgentsConfig>> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("failed to read '{}'", path.display())),
    };

    let config: AgentsConfig = toml::from_str(&content)
        .with_context(|| format!("invalid agents config in '{}'", path.display()))?;

    validate(&config, path)?;
    Ok(Some(config))
}

// ── Validation ─────────────────────────────────────────────────────

fn validate(config: &AgentsConfig, path: &Path) -> Result<()> {
    validate_agent_names(config, path)?;
    validate_team_members(config, path)?;
    validate_isolation_invariants(config, path)?;
    Ok(())
}

/// Agent names must be non-empty, within filesystem-safe length, and unique.
const MAX_AGENT_NAME_LEN: usize = 64;

fn validate_agent_names(config: &AgentsConfig, path: &Path) -> Result<()> {
    let mut seen = HashSet::new();
    for agent in &config.agent_profiles {
        if agent.name.is_empty() {
            bail!("agent name must not be empty in '{}'", path.display());
        }
        if agent.name.len() > MAX_AGENT_NAME_LEN {
            bail!(
                "agent name '{}' exceeds {MAX_AGENT_NAME_LEN}-byte limit in '{}'",
                agent.name,
                path.display()
            );
        }
        if !seen.insert(&agent.name) {
            bail!(
                "duplicate agent name '{}' in '{}'",
                agent.name,
                path.display()
            );
        }
    }
    Ok(())
}

/// Every team member must reference a declared agent profile.
fn validate_team_members(config: &AgentsConfig, path: &Path) -> Result<()> {
    // Safety: `agent_names` borrows from `config` which is held by shared
    // reference for the entire function scope — no mutation is possible.
    let agent_names: HashSet<&str> = config
        .agent_profiles
        .iter()
        .map(|a| a.name.as_str())
        .collect();

    let mut team_names = HashSet::new();
    for team in &config.team_definitions {
        if team.name.is_empty() {
            bail!("team name must not be empty in '{}'", path.display());
        }
        if !team_names.insert(&team.name) {
            bail!(
                "duplicate team name '{}' in '{}'",
                team.name,
                path.display()
            );
        }
        if team.members.is_empty() {
            bail!(
                "team '{}' must have at least one member in '{}'",
                team.name,
                path.display()
            );
        }
        let mut seen_members = HashSet::new();
        for member in &team.members {
            if !seen_members.insert(member.as_str()) {
                bail!(
                    "team '{}' contains duplicate member '{}' in '{}'",
                    team.name,
                    member,
                    path.display()
                );
            }
            if !agent_names.contains(member.as_str()) {
                bail!(
                    "team '{}' references unknown agent '{}' in '{}'; \
                     known agents: [{}]",
                    team.name,
                    member,
                    path.display(),
                    config
                        .agent_profiles
                        .iter()
                        .map(|a| a.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
        }
    }
    Ok(())
}

/// Capabilities that mutate repository state — agents with shared isolation
/// must not declare any of these (ADR-034 §3: "only permitted for read-only
/// tasks").
const MUTABLE_CAPABILITIES: &[&str] = &["apply-patch", "run-command", "write-file"];

/// Shared isolation is only valid when max_parallel_tasks == 1 **and** the
/// agent declares no mutable capabilities (read-only access guarantee per
/// ADR-034 §3).
fn validate_isolation_invariants(config: &AgentsConfig, path: &Path) -> Result<()> {
    for agent in &config.agent_profiles {
        if agent.isolation == IsolationPolicy::Shared {
            if agent.max_parallel_tasks > 1 {
                bail!(
                    "agent '{}' uses shared isolation but max_parallel_tasks={} in '{}'; \
                     shared isolation requires max_parallel_tasks=1",
                    agent.name,
                    agent.max_parallel_tasks,
                    path.display(),
                );
            }
            for cap in &agent.allowed_capabilities {
                if MUTABLE_CAPABILITIES.contains(&cap.as_str()) {
                    bail!(
                        "agent '{}' uses shared isolation but declares mutable capability \
                         '{}' in '{}'; shared isolation is read-only per ADR-034 §3",
                        agent.name,
                        cap,
                        path.display(),
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_config(dir: &Path, content: &str) -> PathBuf {
        let vex_dir = dir.join(".vex");
        std::fs::create_dir_all(&vex_dir).unwrap();
        let path = vex_dir.join("agents.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn parse_minimal_config() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            r#"
[[agents]]
name = "fixer"
"#,
        );
        let config = load_agents_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.agent_profiles.len(), 1);
        assert_eq!(config.agent_profiles[0].name, "fixer");
        assert_eq!(config.agent_profiles[0].profile, "default");
        assert_eq!(
            config.agent_profiles[0].isolation,
            IsolationPolicy::Worktree
        );
        assert_eq!(config.agent_profiles[0].max_parallel_tasks, 1);
        assert!(config.team_definitions.is_empty());
    }

    #[test]
    fn parse_full_config() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            r#"
[[agents]]
name = "rust-fixer"
profile = "default"
isolation = "worktree"
max_parallel_tasks = 2
allowed_capabilities = ["read-file", "apply-patch", "run-command"]

[[agents]]
name = "docs-reviewer"
profile = "review"
isolation = "shared"
allowed_capabilities = ["read-file"]

[[teams]]
name = "parallel-review"
members = ["rust-fixer", "docs-reviewer"]
scheduler = "fan_out_join"
"#,
        );
        let config = load_agents_config(dir.path()).unwrap().unwrap();
        assert_eq!(config.agent_profiles.len(), 2);
        assert_eq!(config.team_definitions.len(), 1);
        assert_eq!(
            config.team_definitions[0].members,
            vec!["rust-fixer", "docs-reviewer"]
        );
        assert_eq!(
            config.team_definitions[0].scheduler,
            TeamScheduler::FanOutJoin
        );
    }

    #[test]
    fn returns_none_when_no_config() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_agents_config(dir.path()).unwrap().is_none());
    }

    #[test]
    fn rejects_empty_agent_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
[[agents]]
name = ""
"#,
        );
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("agent name must not be empty"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn rejects_duplicate_agent_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
[[agents]]
name = "a"

[[agents]]
name = "a"
"#,
        );
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("duplicate agent name"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn rejects_unknown_team_member() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
[[agents]]
name = "a"

[[teams]]
name = "t"
members = ["a", "ghost"]
"#,
        );
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("unknown agent 'ghost'"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn rejects_duplicate_team_member() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
[[agents]]
name = "a"

[[teams]]
name = "t"
members = ["a", "a"]
"#,
        );
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("contains duplicate member 'a'"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn rejects_shared_with_parallel_greater_than_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
[[agents]]
name = "bad"
isolation = "shared"
max_parallel_tasks = 3
"#,
        );
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string()
                .contains("shared isolation requires max_parallel_tasks=1"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn rejects_shared_with_mutable_capability() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
[[agents]]
name = "bad"
isolation = "shared"
allowed_capabilities = ["read-file", "apply-patch"]
"#,
        );
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("shared isolation is read-only"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn rejects_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
[[agents]]
name = "a"
bogus_field = true
"#,
        );
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("invalid agents config"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn rejects_overly_long_agent_name() {
        let dir = tempfile::tempdir().unwrap();
        let long_name = "a".repeat(MAX_AGENT_NAME_LEN + 1);
        let path = write_config(
            dir.path(),
            &format!(
                r#"
[[agents]]
name = "{long_name}"
"#,
            ),
        );
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("exceeds"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn ancestor_walk_finds_config() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "[[agents]]\nname = \"walker\"\n");
        let nested = dir.path().join("deeply").join("nested").join("dir");
        std::fs::create_dir_all(&nested).unwrap();
        let config = load_agents_config(&nested).unwrap().unwrap();
        assert_eq!(config.agent_profiles[0].name, "walker");
    }

    #[test]
    fn rejects_empty_team() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
[[agents]]
name = "a"

[[teams]]
name = "empty-team"
members = []
"#,
        );
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("must have at least one member"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn sequential_scheduler_parses() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            r#"
[[agents]]
name = "a"

[[agents]]
name = "b"

[[teams]]
name = "seq-team"
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
}
