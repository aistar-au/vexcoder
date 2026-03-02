use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    ReadFile,
    WriteFile,
    ApplyPatch,
    RunCommand,
    Network,
    Browser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl FileApprovalPolicy {
    pub fn default() -> Self {
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
        Self { rules }
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
        Self::default()
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

        let mut rules = HashMap::new();

        // Simple TOML-like parsing for capability rules
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim().trim_matches('"').trim_matches('\'');

                let capability = match key {
                    "read_file" => Some(Capability::ReadFile),
                    "write_file" => Some(Capability::WriteFile),
                    "apply_patch" => Some(Capability::ApplyPatch),
                    "run_command" => Some(Capability::RunCommand),
                    "network" => Some(Capability::Network),
                    "browser" => Some(Capability::Browser),
                    _ => None,
                };

                if let Some(cap) = capability {
                    if let Some(action) = Self::parse_toml_value(value) {
                        rules.insert(cap, action);
                    }
                }
            }
        }

        // Fill in defaults for unspecified capabilities
        if !rules.contains_key(&Capability::ReadFile) {
            rules.insert(Capability::ReadFile, PolicyAction::Allow);
        }
        for cap in [
            Capability::WriteFile,
            Capability::ApplyPatch,
            Capability::RunCommand,
            Capability::Network,
            Capability::Browser,
        ] {
            if !rules.contains_key(&cap) {
                rules.insert(cap, PolicyAction::Prompt(ApprovalScope::Once));
            }
        }

        Ok(Self { rules })
    }
}

/// Load policy from VEX_POLICY_FILE env var (default: .vex/policy.toml)
pub fn load_policy_from_env() -> FileApprovalPolicy {
    let policy_path =
        std::env::var("VEX_POLICY_FILE").unwrap_or_else(|_| ".vex/policy.toml".to_string());

    let path = Path::new(&policy_path);
    if path.exists() {
        FileApprovalPolicy::load_from_file(path).unwrap_or_else(|_| FileApprovalPolicy::default())
    } else {
        FileApprovalPolicy::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
