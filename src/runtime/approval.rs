use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    ReadFile,
    WriteFile,
    ApplyPatch,
    RunCommand,
    Network,
    Browser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalScope {
    Once,
    Task,
    Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyAction {
    Allow,
    Prompt(ApprovalScope),
    Deny,
}

pub trait ApprovalPolicy {
    fn evaluate(&self, capability: Capability) -> PolicyAction;
    fn load_from_file(path: &Path) -> Result<Self>
    where
        Self: Sized;
}

#[derive(Debug, Clone)]
pub struct FileApprovalPolicy {
    rules: HashMap<Capability, PolicyAction>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyConfig {
    #[serde(default)]
    capabilities: CapabilityTable,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapabilityTable {
    #[serde(default, rename = "ReadFile", alias = "read_file")]
    read_file: Option<String>,
    #[serde(default, rename = "WriteFile", alias = "write_file")]
    write_file: Option<String>,
    #[serde(default, rename = "ApplyPatch", alias = "apply_patch")]
    apply_patch: Option<String>,
    #[serde(default, rename = "RunCommand", alias = "run_command")]
    run_command: Option<String>,
    #[serde(default, rename = "Network", alias = "network")]
    network: Option<String>,
    #[serde(default, rename = "Browser", alias = "browser")]
    browser: Option<String>,
}

impl FileApprovalPolicy {
    fn default_rules() -> HashMap<Capability, PolicyAction> {
        let mut rules = HashMap::new();
        // Default policy: ReadFile -> Allow, all others -> Prompt(Once)
        rules.insert(Capability::ReadFile, PolicyAction::Allow);
        rules.insert(
            Capability::WriteFile,
            PolicyAction::Prompt(ApprovalScope::Once),
        );
        rules.insert(
            Capability::ApplyPatch,
            PolicyAction::Prompt(ApprovalScope::Once),
        );
        rules.insert(
            Capability::RunCommand,
            PolicyAction::Prompt(ApprovalScope::Once),
        );
        rules.insert(
            Capability::Network,
            PolicyAction::Prompt(ApprovalScope::Once),
        );
        rules.insert(
            Capability::Browser,
            PolicyAction::Prompt(ApprovalScope::Once),
        );
        rules
    }

    fn parse_toml_value(value: &str) -> Option<PolicyAction> {
        match value.trim().to_ascii_lowercase().as_str() {
            "allow" => Some(PolicyAction::Allow),
            "deny" => Some(PolicyAction::Deny),
            "once" => Some(PolicyAction::Prompt(ApprovalScope::Once)),
            "task" => Some(PolicyAction::Prompt(ApprovalScope::Task)),
            "session" => Some(PolicyAction::Prompt(ApprovalScope::Session)),
            _ => None,
        }
    }
}

impl Default for FileApprovalPolicy {
    fn default() -> Self {
        Self {
            rules: Self::default_rules(),
        }
    }
}

impl ApprovalPolicy for FileApprovalPolicy {
    fn evaluate(&self, capability: Capability) -> PolicyAction {
        self.rules
            .get(&capability)
            .copied()
            .unwrap_or(PolicyAction::Prompt(ApprovalScope::Once))
    }

    fn load_from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read policy file: {}", path.display()))?;
        let config = toml::from_str::<PolicyConfig>(&content)
            .with_context(|| format!("Failed to parse policy TOML: {}", path.display()))?;

        let mut policy = Self::default();
        for (capability, maybe_value) in [
            (
                Capability::ReadFile,
                config.capabilities.read_file.as_deref(),
            ),
            (
                Capability::WriteFile,
                config.capabilities.write_file.as_deref(),
            ),
            (
                Capability::ApplyPatch,
                config.capabilities.apply_patch.as_deref(),
            ),
            (
                Capability::RunCommand,
                config.capabilities.run_command.as_deref(),
            ),
            (Capability::Network, config.capabilities.network.as_deref()),
            (Capability::Browser, config.capabilities.browser.as_deref()),
        ] {
            if let Some(action) = maybe_value.and_then(Self::parse_toml_value) {
                policy.rules.insert(capability, action);
            }
        }

        Ok(policy)
    }
}

/// Load policy from VEX_POLICY_FILE env var (default: .vex/policy.toml)
pub fn load_policy_from_env() -> FileApprovalPolicy {
    let policy_path =
        std::env::var("VEX_POLICY_FILE").unwrap_or_else(|_| ".vex/policy.toml".to_string());

    let path = Path::new(&policy_path);
    if path.exists() {
        match FileApprovalPolicy::load_from_file(path) {
            Ok(policy) => policy,
            Err(err) => {
                eprintln!("[policy] failed to load {}: {err:#}", path.display());
                eprintln!("[policy] using default policy");
                FileApprovalPolicy::default()
            }
        }
    } else {
        FileApprovalPolicy::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_approval_policy_read_file_auto_allows_without_prompt() {
        let policy = FileApprovalPolicy::default();
        assert!(matches!(
            policy.evaluate(Capability::ReadFile),
            PolicyAction::Allow
        ));
        assert!(matches!(
            policy.evaluate(Capability::ApplyPatch),
            PolicyAction::Prompt(_)
        ));
        assert!(matches!(
            policy.evaluate(Capability::RunCommand),
            PolicyAction::Prompt(_)
        ));
        let patch_grant = PolicyAction::Prompt(ApprovalScope::Once);
        let _ = patch_grant;
    }

    #[test]
    fn test_policy_parse_toml_values() {
        assert_eq!(
            FileApprovalPolicy::parse_toml_value("allow"),
            Some(PolicyAction::Allow)
        );
        assert_eq!(
            FileApprovalPolicy::parse_toml_value("deny"),
            Some(PolicyAction::Deny)
        );
        assert_eq!(
            FileApprovalPolicy::parse_toml_value("once"),
            Some(PolicyAction::Prompt(ApprovalScope::Once))
        );
        assert_eq!(
            FileApprovalPolicy::parse_toml_value("task"),
            Some(PolicyAction::Prompt(ApprovalScope::Task))
        );
        assert_eq!(
            FileApprovalPolicy::parse_toml_value("session"),
            Some(PolicyAction::Prompt(ApprovalScope::Session))
        );
    }

    #[test]
    fn test_load_from_file_parses_single_quoted_toml_values() {
        let workspace = tempfile::tempdir().expect("tempdir");
        let policy_path = workspace.path().join("policy.toml");
        fs::write(
            &policy_path,
            "[capabilities]\nReadFile = 'allow'\nWriteFile = 'task'\nNetwork = 'deny'\n",
        )
        .expect("write policy");

        let policy = FileApprovalPolicy::load_from_file(&policy_path).expect("load policy");
        assert!(matches!(
            policy.evaluate(Capability::ReadFile),
            PolicyAction::Allow
        ));
        assert!(matches!(
            policy.evaluate(Capability::WriteFile),
            PolicyAction::Prompt(ApprovalScope::Task)
        ));
        assert!(matches!(
            policy.evaluate(Capability::Network),
            PolicyAction::Deny
        ));
        assert!(matches!(
            policy.evaluate(Capability::ApplyPatch),
            PolicyAction::Prompt(ApprovalScope::Once)
        ));
    }

    #[test]
    fn test_load_from_file_rejects_non_conformant_top_level_capability_keys() {
        let workspace = tempfile::tempdir().expect("tempdir");
        let policy_path = workspace.path().join("policy.toml");
        fs::write(&policy_path, "ReadFile = 'allow'\n").expect("write policy");

        let err = FileApprovalPolicy::load_from_file(&policy_path).expect_err("must fail");
        assert!(
            err.to_string().contains("Failed to parse policy TOML"),
            "expected parse failure, got: {err:#}"
        );
    }
}
