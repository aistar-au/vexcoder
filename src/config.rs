use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::util::is_local_endpoint_url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub model_token: Option<String>,
    pub model_name: String,
    pub model_url: String,
    pub working_dir: PathBuf,
}

impl Config {
    pub fn load() -> Result<Self> {
        let model_url = std::env::var("VEX_MODEL_URL").map_err(|_| {
            anyhow::anyhow!("VEX_MODEL_URL must be set (e.g. http://localhost:8000/v1/messages)")
        })?;
        let model_token = std::env::var("VEX_MODEL_TOKEN").ok().and_then(|v| {
            if v.trim().is_empty() {
                None
            } else {
                Some(v)
            }
        });
        let model_name =
            std::env::var("VEX_MODEL_NAME").unwrap_or_else(|_| "local/default".to_string());

        Ok(Self {
            model_token,
            model_name,
            model_url,
            working_dir: std::env::current_dir()?,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if !self.model_url.starts_with("http://") && !self.model_url.starts_with("https://") {
            bail!(
                "Invalid VEX_MODEL_URL '{}': expected http:// or https:// URL",
                self.model_url
            );
        }

        if self.model_name.trim().is_empty() {
            bail!("VEX_MODEL_NAME must not be empty");
        }

        let local_endpoint = self.is_local_endpoint();
        if !local_endpoint && self.model_token.is_none() {
            bail!(
                "VEX_MODEL_TOKEN must be set for non-local endpoints (url: '{}')",
                self.model_url
            );
        }

        if !local_endpoint && self.model_name.starts_with("local/") {
            bail!("Local models are only allowed for localhost endpoints");
        }

        Ok(())
    }

    fn is_local_endpoint(&self) -> bool {
        is_local_endpoint_url(&self.model_url)
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn test_config_loads_vex_model_name_without_claude_prefix() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
        std::env::set_var("VEX_MODEL_NAME", "llama-3-70b");
        std::env::remove_var("VEX_MODEL_TOKEN");

        let cfg = Config::load().expect("load failed");
        let result = cfg.validate();

        std::env::remove_var("VEX_MODEL_URL");
        std::env::remove_var("VEX_MODEL_NAME");

        assert!(
            result.is_ok(),
            "neutral model name must pass validation: {:?}",
            result
        );
    }
}
