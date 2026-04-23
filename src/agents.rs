use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentsConfig {
    #[serde(default, rename = "agents")]
    pub agent_profiles: Vec<AgentProfile>,
    #[serde(default, rename = "teams")]
    pub team_definitions: Vec<TeamDefinition>,
}

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

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IsolationPolicy {
    #[default]
    Worktree,

    Shared,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TeamScheduler {
    #[default]
    FanOutJoin,

    Sequential,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamDefinition {
    pub name: String,
    pub members: Vec<String>,
    #[serde(default)]
    pub scheduler: TeamScheduler,
}

fn default_profile() -> String {
    "default".to_string()
}

fn default_max_parallel() -> u32 {
    1
}

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

pub fn load_agents_config(cwd: &Path) -> Result<Option<AgentsConfig>> {
    let path = match find_agents_config(cwd) {
        Some(p) => p,
        None => return Ok(None),
    };
    load_agents_config_from_path(&path)
}

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

fn validate(config: &AgentsConfig, path: &Path) -> Result<()> {
    validate_agent_names(config, path)?;
    validate_team_members(config, path)?;
    validate_isolation_invariants(config, path)?;
    Ok(())
}

const MAX_CONFIG_NAME_LEN: usize = 64;
const WINDOWS_RESERVED_DEVICE_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn is_filesystem_safe(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." || name.ends_with([' ', '.']) {
        return false;
    }
    if name.chars().any(|ch| {
        ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
    }) {
        return false;
    }

    let stem = name.split('.').next().unwrap_or(name);
    !WINDOWS_RESERVED_DEVICE_NAMES.contains(&stem.to_ascii_uppercase().as_str())
}

fn escaped_name(name: &str) -> String {
    name.escape_default().collect()
}

fn validate_agent_names(config: &AgentsConfig, path: &Path) -> Result<()> {
    let mut seen = HashSet::new();
    for agent in &config.agent_profiles {
        if agent.name.is_empty() {
            bail!("agent name must not be empty in '{}'", path.display());
        }
        if agent.name.len() > MAX_CONFIG_NAME_LEN {
            bail!(
                "agent name '{}' exceeds {MAX_CONFIG_NAME_LEN}-byte limit in '{}'",
                escaped_name(&agent.name),
                path.display()
            );
        }
        if !is_filesystem_safe(&agent.name) {
            bail!(
                "agent name '{}' contains characters unsafe for filesystem paths in '{}'",
                escaped_name(&agent.name),
                path.display()
            );
        }
        if !seen.insert(&agent.name) {
            bail!(
                "duplicate agent name '{}' in '{}'",
                escaped_name(&agent.name),
                path.display()
            );
        }
    }
    Ok(())
}

fn validate_team_members(config: &AgentsConfig, path: &Path) -> Result<()> {
    // Safety: `agent_names` borrows from `config` which is held by shared

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
        if team.name.len() > MAX_CONFIG_NAME_LEN {
            bail!(
                "team name '{}' exceeds {MAX_CONFIG_NAME_LEN}-byte limit in '{}'",
                escaped_name(&team.name),
                path.display()
            );
        }
        if !is_filesystem_safe(&team.name) {
            bail!(
                "team name '{}' contains characters unsafe for filesystem paths in '{}'",
                escaped_name(&team.name),
                path.display()
            );
        }
        if !team_names.insert(&team.name) {
            bail!(
                "duplicate team name '{}' in '{}'",
                escaped_name(&team.name),
                path.display()
            );
        }
        if team.members.is_empty() {
            bail!(
                "team '{}' must have at least one member in '{}'",
                escaped_name(&team.name),
                path.display()
            );
        }
        let mut seen_members = HashSet::new();
        for member in &team.members {
            if !seen_members.insert(member.as_str()) {
                bail!(
                    "team '{}' contains duplicate member '{}' in '{}'",
                    escaped_name(&team.name),
                    escaped_name(member),
                    path.display()
                );
            }
            if !agent_names.contains(member.as_str()) {
                bail!(
                    "team '{}' references unknown agent '{}' in '{}'; \
                     known agents: [{}]",
                    escaped_name(&team.name),
                    escaped_name(member),
                    path.display(),
                    config
                        .agent_profiles
                        .iter()
                        .map(|a| escaped_name(&a.name))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
        }
    }
    Ok(())
}

const MUTABLE_CAPABILITIES: &[&str] = &["apply-patch", "run-command", "write-file"];

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
        let long_name = "a".repeat(MAX_CONFIG_NAME_LEN + 1);
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
    fn rejects_overly_long_team_name() {
        let dir = tempfile::tempdir().unwrap();
        let long_name = "a".repeat(MAX_CONFIG_NAME_LEN + 1);
        let path = write_config(
            dir.path(),
            &format!(
                "[[agents]]\nname = \"a\"\n\n[[teams]]\nname = \"{long_name}\"\nmembers = [\"a\"]\n",
            ),
        );
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("team name"),
            "unexpected error: {err}",
        );
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

    #[test]
    fn rejects_agent_name_with_path_separator() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), "[[agents]]\nname = \"../escape\"\n");
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("unsafe for filesystem"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn rejects_agent_name_with_backslash() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), "[[agents]]\nname = \"back\\\\slash\"\n");
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("unsafe for filesystem"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn rejects_agent_name_with_control_char() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), "[[agents]]\nname = \"bad\\u0001name\"\n");
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("unsafe for filesystem"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn rejects_agent_name_with_unicode_control_char() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), "[[agents]]\nname = \"bad\\u0085name\"\n");
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("unsafe for filesystem"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn rejects_dot_dot_agent_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), "[[agents]]\nname = \"..\"\n");
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("unsafe for filesystem"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn rejects_dot_agent_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), "[[agents]]\nname = \".\"\n");
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("unsafe for filesystem"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn rejects_team_name_with_path_separator() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            "[[agents]]\nname = \"a\"\n\n[[teams]]\nname = \"bad/team\"\nmembers = [\"a\"]\n",
        );
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("unsafe for filesystem"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn rejects_windows_reserved_device_name() {
        assert!(!super::is_filesystem_safe("COM1"));
        assert!(!super::is_filesystem_safe("nul.txt"));
    }

    #[test]
    fn rejects_windows_invalid_suffixes() {
        assert!(!super::is_filesystem_safe("trailing."));
        assert!(!super::is_filesystem_safe("trailing "));
    }

    #[test]
    fn error_messages_escape_control_characters() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), "[[agents]]\nname = \"bad\\u0001name\"\n");
        let err = load_agents_config_from_path(&path).unwrap_err();
        assert!(
            err.to_string().contains("bad\\u{1}name"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn is_filesystem_safe_accepts_valid_names() {
        assert!(super::is_filesystem_safe("fixer"));
        assert!(super::is_filesystem_safe("rust-fixer-01"));
        assert!(super::is_filesystem_safe("my_agent.v2"));
    }

    #[test]
    fn is_filesystem_safe_rejects_dangerous_names() {
        assert!(!super::is_filesystem_safe("."));
        assert!(!super::is_filesystem_safe(".."));
        assert!(!super::is_filesystem_safe("a/b"));
        assert!(!super::is_filesystem_safe("a\\b"));
        assert!(!super::is_filesystem_safe("a\0b"));
        assert!(!super::is_filesystem_safe("a\x01b"));
        assert!(!super::is_filesystem_safe("a:b"));
        assert!(!super::is_filesystem_safe("a?b"));
    }
}
