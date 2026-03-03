use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::runtime::{ModelBackendKind, ModelProtocol, ToolCallMode};
use crate::util::is_local_endpoint_url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub model_token: Option<String>,
    pub model_name: String,
    pub model_url: String,
    pub working_dir: PathBuf,
    pub model_backend: ModelBackendKind,
    pub model_protocol: ModelProtocol,
    pub tool_call_mode: ToolCallMode,
    #[serde(skip)]
    pub model_headers: HeaderMap,
}

impl Config {
    pub fn load() -> Result<Self> {
        let model_url = std::env::var("VEX_MODEL_URL").map_err(|_| {
            anyhow::anyhow!("VEX_MODEL_URL must be set (e.g. http://localhost:<port>/v1/messages)")
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

        let is_local = is_local_endpoint_url(&model_url);

        let model_backend = std::env::var("VEX_MODEL_BACKEND")
            .ok()
            .and_then(parse_model_backend)
            .unwrap_or({
                if is_local {
                    ModelBackendKind::LocalRuntime
                } else {
                    ModelBackendKind::ApiServer
                }
            });

        let model_protocol = std::env::var("VEX_MODEL_PROTOCOL")
            .ok()
            .and_then(parse_model_protocol)
            .unwrap_or_else(|| infer_model_protocol(&model_url));

        let tool_call_mode = std::env::var("VEX_TOOL_CALL_MODE")
            .ok()
            .and_then(parse_tool_call_mode)
            .unwrap_or({
                if is_local {
                    ToolCallMode::TaggedFallback
                } else {
                    ToolCallMode::Structured
                }
            });

        // Read working_dir from VEX_WORKDIR env var, default to current_dir
        let working_dir = if let Some(value) = std::env::var("VEX_WORKDIR")
            .ok()
            .filter(|value| !value.trim().is_empty())
        {
            PathBuf::from(value)
        } else {
            std::env::current_dir().context("Failed to determine current working directory")?
        };

        Ok(Self {
            model_token,
            model_name,
            model_url,
            working_dir,
            model_backend,
            model_protocol,
            tool_call_mode,
            model_headers: parse_model_headers_json()?,
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

fn parse_model_backend(value: String) -> Option<ModelBackendKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "local-runtime" | "local_runtime" | "local" => Some(ModelBackendKind::LocalRuntime),
        "api-server" | "api_server" | "api" | "remote" => Some(ModelBackendKind::ApiServer),
        _ => None,
    }
}

fn parse_model_protocol(value: String) -> Option<ModelProtocol> {
    match value.trim().to_ascii_lowercase().as_str() {
        "messages-v1" | "messages_v1" | "messages" | "v1" => Some(ModelProtocol::MessagesV1),
        "chat-compat" | "chat_compat" | "chat" => Some(ModelProtocol::ChatCompat),
        _ => None,
    }
}

fn parse_tool_call_mode(value: String) -> Option<ToolCallMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "structured" => Some(ToolCallMode::Structured),
        "tagged-fallback" | "tagged_fallback" | "fallback" | "tagged" => {
            Some(ToolCallMode::TaggedFallback)
        }
        _ => None,
    }
}

fn infer_model_protocol(api_url: &str) -> ModelProtocol {
    let normalized = api_url.trim().to_ascii_lowercase();
    if normalized.contains("/chat/completions") || normalized.ends_with("/v1") {
        ModelProtocol::ChatCompat
    } else {
        ModelProtocol::MessagesV1
    }
}

fn parse_model_headers_json() -> Result<HeaderMap> {
    let raw = match std::env::var("VEX_MODEL_HEADERS_JSON") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => return Ok(HeaderMap::new()),
    };
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("VEX_MODEL_HEADERS_JSON is not a valid JSON object: {e}"))?;
    let mut headers = HeaderMap::new();
    for (k, v) in &map {
        let name = HeaderName::from_bytes(k.as_bytes()).map_err(|e| {
            anyhow::anyhow!("VEX_MODEL_HEADERS_JSON invalid header name {k:?}: {e}")
        })?;
        let val_str = v.as_str().ok_or_else(|| {
            anyhow::anyhow!("VEX_MODEL_HEADERS_JSON value for {k:?} must be a string")
        })?;
        let value = HeaderValue::from_str(val_str).map_err(|e| {
            anyhow::anyhow!("VEX_MODEL_HEADERS_JSON invalid header value for {k:?}: {e}")
        })?;
        headers.insert(name, value);
    }
    Ok(headers)
}

#[cfg(test)]
mod tests {
    use super::{Config, ModelBackendKind};

    #[test]
    fn test_config_loads_vex_model_name_without_vendor_prefix() {
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

    #[test]
    fn test_model_backend_kind_parses_from_env_var() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var("VEX_MODEL_BACKEND", "local-runtime");
        std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
        std::env::set_var("VEX_MODEL_NAME", "local-model");
        let cfg = Config::load().expect("load failed");
        assert!(cfg.validate().is_ok());
        assert_eq!(cfg.model_backend, ModelBackendKind::LocalRuntime);
        std::env::remove_var("VEX_MODEL_BACKEND");
        std::env::remove_var("VEX_MODEL_URL");
        std::env::remove_var("VEX_MODEL_NAME");
    }

    #[test]
    fn test_parse_model_headers_json_valid() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var(
            "VEX_MODEL_HEADERS_JSON",
            r#"{"x-custom-header": "value1", "x-other": "value2"}"#,
        );
        let headers = super::parse_model_headers_json().unwrap();
        assert_eq!(headers.len(), 2);
        std::env::remove_var("VEX_MODEL_HEADERS_JSON");
    }

    #[test]
    fn test_parse_model_headers_json_invalid_name_rejected() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var("VEX_MODEL_HEADERS_JSON", r#"{"invalid header!": "v"}"#);
        assert!(super::parse_model_headers_json().is_err());
        std::env::remove_var("VEX_MODEL_HEADERS_JSON");
    }

    #[test]
    fn test_parse_model_headers_json_non_string_value_rejected() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var("VEX_MODEL_HEADERS_JSON", r#"{"x-count": 42}"#);
        assert!(super::parse_model_headers_json().is_err());
        std::env::remove_var("VEX_MODEL_HEADERS_JSON");
    }

    #[test]
    fn test_parse_model_headers_json_empty_env_returns_empty_map() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::remove_var("VEX_MODEL_HEADERS_JSON");
        let headers = super::parse_model_headers_json().unwrap();
        assert!(headers.is_empty());
    }
}
