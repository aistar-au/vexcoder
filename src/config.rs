use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

/// Intermediate per-layer config built from a TOML file.
/// `deny_unknown_fields` ensures any unrecognized key is a hard failure.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ConfigLayer {
    model_name: Option<String>,
    model_url: Option<String>,
    working_dir: Option<PathBuf>,
    model_backend: Option<String>,
    model_protocol: Option<String>,
    tool_call_mode: Option<String>,
}

impl Config {
    /// Load config from the five-layer resolution chain.
    ///
    /// Precedence (highest → lowest):
    ///   environment > repo-local `.vex/config.toml` > user > system > compiled defaults
    ///
    /// Repo-local discovery walks ancestors of `std::env::current_dir()`.
    /// Missing files are silently ignored. Malformed TOML, unknown keys,
    /// invalid enum values, and `model_token` in any file are hard failures
    /// with file-path context in the error message.
    pub fn load() -> Result<Self> {
        let cwd =
            std::env::current_dir().context("Failed to determine current working directory")?;
        let repo_cfg = find_repo_local_config(&cwd);
        let user_cfg = user_config_path();
        let system_cfg = system_config_path();
        Self::load_layers(
            &cwd,
            repo_cfg.as_deref(),
            user_cfg.as_deref(),
            system_cfg.as_deref(),
        )
    }

    /// Test-only helper. Accepts explicit user and system config paths so
    /// tests can inject fixtures without touching the operator's real home
    /// directory or `/etc`. Repo-local config is still discovered by walking
    /// ancestors of `cwd`.
    pub fn load_for_tests(cwd: &Path, user: Option<&Path>, system: Option<&Path>) -> Result<Self> {
        let repo_cfg = find_repo_local_config(cwd);
        Self::load_layers(cwd, repo_cfg.as_deref(), user, system)
    }

    fn load_layers(
        cwd: &Path,
        repo_cfg: Option<&Path>,
        user_cfg: Option<&Path>,
        system_cfg: Option<&Path>,
    ) -> Result<Self> {
        // Load file layers; missing files yield None, present-but-invalid files error.
        let system_layer = system_cfg.map(load_config_layer).transpose()?.flatten();
        let user_layer = user_cfg.map(load_config_layer).transpose()?.flatten();
        let repo_layer = repo_cfg.map(load_config_layer).transpose()?.flatten();

        // Env layer is parsed separately so errors name the env var, not a file.
        let (env_layer, env_token) = read_env_layer()?;

        // Merge: system ← user ← repo ← env  (higher wins).
        let mut merged = ConfigLayer::default();
        if let Some(l) = system_layer {
            merged = apply_over(merged, l);
        }
        if let Some(l) = user_layer {
            merged = apply_over(merged, l);
        }
        if let Some(l) = repo_layer {
            merged = apply_over(merged, l);
        }
        merged = apply_over(merged, env_layer);

        resolve_config(merged, env_token, cwd)
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

// ---------------------------------------------------------------------------
// Layer helpers
// ---------------------------------------------------------------------------

/// Apply `over` on top of `base`: any Some field in `over` wins.
fn apply_over(base: ConfigLayer, over: ConfigLayer) -> ConfigLayer {
    ConfigLayer {
        model_name: over.model_name.or(base.model_name),
        model_url: over.model_url.or(base.model_url),
        working_dir: over.working_dir.or(base.working_dir),
        model_backend: over.model_backend.or(base.model_backend),
        model_protocol: over.model_protocol.or(base.model_protocol),
        tool_call_mode: over.tool_call_mode.or(base.tool_call_mode),
    }
}

/// Read environment variables into a ConfigLayer and return the env token
/// separately (token is forbidden in file layers).
///
/// VEX_MODEL_PROTOCOL is validated here so the error message names the env var.
fn read_env_layer() -> Result<(ConfigLayer, Option<String>)> {
    let env_token = std::env::var("VEX_MODEL_TOKEN").ok().and_then(|v| {
        if v.trim().is_empty() {
            None
        } else {
            Some(v)
        }
    });

    let model_protocol = match std::env::var("VEX_MODEL_PROTOCOL") {
        Ok(v) if !v.trim().is_empty() => {
            if parse_model_protocol(v.clone()).is_none() {
                bail!(
                    "Invalid VEX_MODEL_PROTOCOL '{}': expected one of \
                     messages-v1, messages_v1, messages, v1, chat-compat, chat_compat, chat",
                    v
                );
            }
            Some(v)
        }
        _ => None,
    };

    let model_backend = match std::env::var("VEX_MODEL_BACKEND") {
        Ok(v) if !v.trim().is_empty() => {
            if parse_model_backend(v.clone()).is_none() {
                bail!(
                    "Invalid VEX_MODEL_BACKEND '{}': expected one of \
                     local-runtime, local_runtime, local, api-server, api_server, api",
                    v
                );
            }
            Some(v)
        }
        _ => None,
    };

    let tool_call_mode = match std::env::var("VEX_TOOL_CALL_MODE") {
        Ok(v) if !v.trim().is_empty() => {
            if parse_tool_call_mode(v.clone()).is_none() {
                bail!(
                    "Invalid VEX_TOOL_CALL_MODE '{}': expected one of \
                     structured, structured-tool-calls, structured_tool_calls, \
                     tagged-fallback, tagged_fallback, tagged",
                    v
                );
            }
            Some(v)
        }
        _ => None,
    };

    let layer = ConfigLayer {
        model_name: std::env::var("VEX_MODEL_NAME")
            .ok()
            .filter(|v| !v.trim().is_empty()),
        model_url: std::env::var("VEX_MODEL_URL")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        working_dir: std::env::var("VEX_WORKDIR")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from),
        model_backend,
        model_protocol,
        tool_call_mode,
    };

    Ok((layer, env_token))
}

/// Load and validate a single TOML config file.
///
/// Returns `Ok(None)` when the file does not exist (not an error).
/// Returns `Err` for: `model_token` present, unknown keys, malformed TOML,
/// or invalid enum string values — all with the file path in the message.
fn load_config_layer(path: &Path) -> Result<Option<ConfigLayer>> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(e)
                .with_context(|| format!("failed to read config file '{}'", path.display()));
        }
    };

    // First pass: parse to toml::Value to check for model_token before the
    // typed parse so the diagnostic names the file, not a generic serde error.
    let raw: toml::Value = toml::from_str(&content)
        .with_context(|| format!("malformed TOML in '{}'", path.display()))?;
    if raw.get("model_token").is_some() {
        bail!(
            "'model_token' found in '{}': this key must not appear in config \
             files; set VEX_MODEL_TOKEN via the environment only",
            path.display()
        );
    }

    // Second pass: typed parse with deny_unknown_fields.
    let layer: ConfigLayer = toml::from_str(&content)
        .with_context(|| format!("unknown or invalid key in config file '{}'", path.display()))?;

    // Validate enum string values here so errors carry file-path context.
    if let Some(ref s) = layer.model_backend {
        if parse_model_backend(s.clone()).is_none() {
            bail!(
                "config file '{}': invalid model_backend '{}': expected one of \
                 local-runtime, local_runtime, local, api-server, api_server, api",
                path.display(),
                s
            );
        }
    }
    if let Some(ref s) = layer.model_protocol {
        if parse_model_protocol(s.clone()).is_none() {
            bail!(
                "config file '{}': invalid model_protocol '{}': expected one of \
                 messages-v1, messages_v1, messages, v1, chat-compat, chat_compat, chat",
                path.display(),
                s
            );
        }
    }
    if let Some(ref s) = layer.tool_call_mode {
        if parse_tool_call_mode(s.clone()).is_none() {
            bail!(
                "config file '{}': invalid tool_call_mode '{}': expected one of \
                 structured, structured-tool-calls, structured_tool_calls, \
                 tagged-fallback, tagged_fallback, tagged",
                path.display(),
                s
            );
        }
    }

    Ok(Some(layer))
}

/// Resolve a fully-merged ConfigLayer into a concrete Config.
/// Compiled defaults fill any field not set by any layer.
/// `fallback_cwd` is used as the default `working_dir` when no layer sets it.
fn resolve_config(
    merged: ConfigLayer,
    env_token: Option<String>,
    fallback_cwd: &Path,
) -> Result<Config> {
    let model_url = merged
        .model_url
        .unwrap_or_else(|| "http://localhost:11434/v1".to_string());
    let model_name = merged
        .model_name
        .unwrap_or_else(|| "local/default".to_string());
    let working_dir = merged
        .working_dir
        .unwrap_or_else(|| fallback_cwd.to_path_buf());

    let is_local = is_local_endpoint_url(&model_url);

    let model_backend = merged
        .model_backend
        .and_then(parse_model_backend)
        .unwrap_or(if is_local {
            ModelBackendKind::LocalRuntime
        } else {
            ModelBackendKind::ApiServer
        });

    let model_protocol = merged
        .model_protocol
        .and_then(parse_model_protocol)
        .unwrap_or_else(|| infer_model_protocol(&model_url));

    let tool_call_mode = merged
        .tool_call_mode
        .and_then(parse_tool_call_mode)
        .unwrap_or(if is_local {
            ToolCallMode::TaggedFallback
        } else {
            ToolCallMode::Structured
        });

    Ok(Config {
        model_token: env_token,
        model_name,
        model_url,
        working_dir,
        model_backend,
        model_protocol,
        tool_call_mode,
        model_headers: parse_model_headers_json()?,
    })
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Walk ancestors of `cwd` to find the nearest `.vex/config.toml`.
/// The resolved `working_dir` from the merged config must not influence
/// which file is selected — always walk from the actual process cwd.
fn find_repo_local_config(cwd: &Path) -> Option<PathBuf> {
    let mut dir: &Path = cwd;
    loop {
        let candidate = dir.join(".vex").join("config.toml");
        if candidate.exists() {
            return Some(candidate);
        }
        dir = dir.parent()?;
    }
}

fn user_config_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|home| {
            PathBuf::from(home)
                .join(".config")
                .join("vex")
                .join("config.toml")
        })
}

fn system_config_path() -> Option<PathBuf> {
    Some(PathBuf::from("/etc/vex/config.toml"))
}

// ---------------------------------------------------------------------------
// Parse helpers (preserved from original)
// ---------------------------------------------------------------------------

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
    fn test_config_loads_vex_model_name_without_claude_prefix() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
        std::env::set_var("VEX_MODEL_NAME", "llama-3-70b");
        std::env::remove_var("VEX_MODEL_TOKEN");

        let cfg = Config::load().expect("load failed");
        assert!(
            cfg.validate().is_ok(),
            "neutral model name must pass validation"
        );
        std::env::remove_var("VEX_MODEL_URL");
        std::env::remove_var("VEX_MODEL_NAME");
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
    fn test_invalid_model_protocol_env_var_is_rejected() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
        std::env::set_var("VEX_MODEL_NAME", "mock-model");
        std::env::set_var("VEX_MODEL_PROTOCOL", "legacy-value");

        assert!(Config::load().is_err());

        std::env::remove_var("VEX_MODEL_URL");
        std::env::remove_var("VEX_MODEL_NAME");
        std::env::remove_var("VEX_MODEL_PROTOCOL");
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
