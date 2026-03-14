use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::prompts::CODER_SYSTEM_PROMPT;
use crate::runtime::{ModelBackendKind, ToolCallMode};

const CODER_SYSTEM_PROMPT_PATH: &str = "src/prompts/coder_system.txt";

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelProfile {
    pub name: String,
    pub system_prompt: PathBuf,
    pub temperature: f32,
    pub top_p: f32,
    pub max_tokens: u32,
    pub stop_sequences: Vec<String>,
    pub structured_tools: bool,
    pub reasoning_budget: u32,
}

impl ModelProfile {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read model profile '{}'", path.display()))?;
        let profile: Self = toml::from_str(&raw)
            .with_context(|| format!("failed to parse model profile '{}'", path.display()))?;
        profile
            .validated()
            .with_context(|| format!("invalid model profile '{}'", path.display()))
    }

    pub fn default_for_backend(backend: ModelBackendKind) -> Self {
        Self {
            name: "default".to_string(),
            system_prompt: PathBuf::from(CODER_SYSTEM_PROMPT_PATH),
            temperature: 0.3,
            top_p: 1.0,
            max_tokens: match backend {
                ModelBackendKind::LocalRuntime => 1024,
                ModelBackendKind::ApiServer => 4096,
            },
            stop_sequences: Vec::new(),
            structured_tools: matches!(backend, ModelBackendKind::ApiServer),
            reasoning_budget: 0,
        }
    }

    pub fn tool_call_mode(&self) -> ToolCallMode {
        if self.structured_tools {
            ToolCallMode::Structured
        } else {
            ToolCallMode::TaggedFallback
        }
    }

    pub fn system_prompt_text(&self) -> Result<&'static str> {
        resolve_system_prompt_text(&self.system_prompt)
    }

    fn validated(self) -> Result<Self> {
        if self.name.trim().is_empty() {
            bail!("profile name must not be empty");
        }
        if self.system_prompt.as_os_str().is_empty() {
            bail!("system_prompt must not be empty");
        }
        if !self.temperature.is_finite() || self.temperature < 0.0 {
            bail!("temperature must be a finite non-negative number");
        }
        if !self.top_p.is_finite() || !(0.0..=1.0).contains(&self.top_p) || self.top_p == 0.0 {
            bail!("top_p must be in the range (0.0, 1.0]");
        }
        if self.max_tokens == 0 {
            bail!("max_tokens must be greater than zero");
        }
        let _ = resolve_system_prompt_text(&self.system_prompt)?;
        Ok(self)
    }
}

fn resolve_system_prompt_text(path: &Path) -> Result<&'static str> {
    match normalize_relative_path(path).as_str() {
        CODER_SYSTEM_PROMPT_PATH => Ok(CODER_SYSTEM_PROMPT),
        normalized => bail!(
            "unsupported system_prompt path '{}': expected '{}'",
            normalized,
            CODER_SYSTEM_PROMPT_PATH
        ),
    }
}

fn normalize_relative_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn test_model_profile_loads_from_toml() {
        let profile = ModelProfile::load(&repo_root().join("models/qwen-coder.toml"))
            .expect("qwen fixture should load");

        assert_eq!(profile.name, "qwen-coder");
        assert_eq!(
            profile.system_prompt,
            PathBuf::from(CODER_SYSTEM_PROMPT_PATH)
        );
        assert_eq!(profile.temperature, 0.2);
        assert_eq!(profile.tool_call_mode(), ToolCallMode::Structured);
        assert!(
            profile
                .system_prompt_text()
                .expect("system prompt must resolve")
                .contains("You are"),
            "expected bundled prompt text"
        );
    }

    #[test]
    fn test_model_profile_invalid_path_is_hard_failure() {
        let error = ModelProfile::load(&repo_root().join("models/does-not-exist.toml"))
            .expect_err("missing profile path must fail");

        assert!(
            error.to_string().contains("does-not-exist.toml"),
            "error should include the invalid path: {error}"
        );
    }

    #[test]
    fn test_model_profile_structured_tools_false_uses_tagged_fallback() {
        let profile = ModelProfile::load(&repo_root().join("models/codellama.toml"))
            .expect("codellama fixture should load");

        assert!(!profile.structured_tools);
        assert_eq!(profile.tool_call_mode(), ToolCallMode::TaggedFallback);
    }

    #[test]
    fn test_default_profile_follows_backend_defaults() {
        assert_eq!(
            ModelProfile::default_for_backend(ModelBackendKind::LocalRuntime).max_tokens,
            1024
        );
        assert_eq!(
            ModelProfile::default_for_backend(ModelBackendKind::ApiServer).max_tokens,
            4096
        );
        assert_eq!(
            ModelProfile::default_for_backend(ModelBackendKind::LocalRuntime).tool_call_mode(),
            ToolCallMode::TaggedFallback
        );
        assert_eq!(
            ModelProfile::default_for_backend(ModelBackendKind::ApiServer).tool_call_mode(),
            ToolCallMode::Structured
        );
    }
}
