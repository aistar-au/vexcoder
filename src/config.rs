use anyhow::{bail, Result};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::runtime::{ModelBackendKind, ModelProtocol, SandboxConfig, ToolCallMode};
use crate::types::ModelProfile;
use crate::util::is_local_endpoint_url;

pub mod hooks;
pub use hooks::{default_hook_on_fail, HookConfig, HookEvent, HookOnFail};

mod load;
#[cfg(test)]
mod tests;

#[cfg(test)]
use self::load::{
    default_model_backend, default_tool_call_mode, infer_model_protocol,
    legacy_chat_protocol_value, legacy_messages_protocol_value, parse_model_headers_json,
    read_env_layer, user_config_path,
};

const DEFAULT_LOCAL_API_HOST: &str = "127.0.0.1";
const DEFAULT_LOCAL_API_PORT: u16 = 6274;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    Stdio,
    Http,
}

impl McpTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransport,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Per-server connection timeout in seconds.  Falls back to
    /// `VEX_MCP_TIMEOUT` env var, then the built-in default (30 s).
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiTransport {
    #[default]
    Http,
    Unix,
    Both,
}

impl ApiTransport {
    pub fn http_enabled(self) -> bool {
        matches!(self, Self::Http | Self::Both)
    }

    pub fn unix_enabled(self) -> bool {
        matches!(self, Self::Unix | Self::Both)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiConfig {
    pub transport: ApiTransport,
    pub host: String,
    pub port: u16,
    pub socket: Option<PathBuf>,
    pub key: Option<String>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub tls_ca_cert: Option<PathBuf>,
    pub tls_skip_verify: bool,
    pub vpn_trust: bool,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            transport: ApiTransport::Http,
            host: DEFAULT_LOCAL_API_HOST.to_string(),
            port: DEFAULT_LOCAL_API_PORT,
            socket: None,
            key: None,
            tls_cert: None,
            tls_key: None,
            tls_ca_cert: None,
            tls_skip_verify: false,
            vpn_trust: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub model_token: Option<String>,
    pub model_name: String,
    pub model_url: String,
    pub model_url_skip_tls_check: bool,
    pub working_dir: PathBuf,
    pub model_backend: ModelBackendKind,
    pub model_protocol: ModelProtocol,
    pub tool_call_mode: ToolCallMode,
    pub model_profile: ModelProfile,
    /// Estimated token budget for project instructions injection (byte len / 4).
    /// Controlled by `VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS`. Default: 4096.
    pub max_project_instructions_tokens: usize,
    /// Estimated token budget for notes injection (byte len / 4).
    /// Controlled by `VEX_MAX_MEMORY_TOKENS`. Default: 2048.
    pub max_memory_tokens: usize,
    pub sandbox: SandboxConfig,
    #[serde(skip)]
    pub model_headers: HeaderMap,
    pub notes_path: Option<PathBuf>,
    pub api: ApiConfig,
    #[serde(default)]
    pub hooks: Vec<HookConfig>,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorConfigSnapshot {
    pub model_url: Option<String>,
    pub working_dir: PathBuf,
    pub model_token_present: bool,
    pub sandbox_require: bool,
    pub mcp_servers: Vec<DoctorMcpServer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorMcpServer {
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub url: Option<String>,
}

/// Intermediate per-layer config built from a TOML file.
/// `deny_unknown_fields` ensures any unrecognized key is a hard failure.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ConfigLayer {
    model_name: Option<String>,
    model_url: Option<String>,
    model_url_skip_tls_check: Option<bool>,
    working_dir: Option<PathBuf>,
    sandbox: Option<String>,
    sandbox_profile: Option<String>,
    sandbox_require: Option<bool>,
    model_backend: Option<String>,
    model_protocol: Option<String>,
    tool_call_mode: Option<String>,
    model_profile: Option<PathBuf>,
    max_project_instructions_tokens: Option<usize>,
    max_memory_tokens: Option<usize>,
    notes_path: Option<PathBuf>,
    api: Option<ApiConfigLayer>,
    hooks: Option<Vec<HookConfig>>,
    mcp_servers: Option<Vec<McpServerConfig>>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ApiConfigLayer {
    transport: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    socket: Option<PathBuf>,
    key: Option<String>,
    tls_cert: Option<PathBuf>,
    tls_key: Option<PathBuf>,
    tls_ca_cert: Option<PathBuf>,
    tls_skip_verify: Option<bool>,
    vpn_trust: Option<bool>,
}

#[derive(Debug, Default)]
struct DoctorConfigLayer {
    model_url: Option<String>,
    working_dir: Option<PathBuf>,
    sandbox_require: Option<bool>,
    mcp_servers: Option<Vec<DoctorMcpServer>>,
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
        load::load()
    }

    /// Test-only helper. Accepts explicit user and system config paths so
    /// tests can inject fixtures without touching the operator's real home
    /// directory or `/etc`. Repo-local config is still discovered by walking
    /// ancestors of `cwd`.
    pub fn load_for_tests(cwd: &Path, user: Option<&Path>, system: Option<&Path>) -> Result<Self> {
        load::load_for_tests(cwd, user, system)
    }

    /// Sensible defaults for interactive TUI startup — used when no config
    /// file or environment variables are present.  Avoids the full five-layer
    /// resolution chain so callers that already hold a `Config` (e.g. tests)
    /// can build a `TuiMode` without side-effects.
    pub fn default_for_tui() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            model_token: None,
            model_name: "local/default".to_string(),
            model_url: String::new(),
            model_url_skip_tls_check: false,
            working_dir: cwd,
            model_backend: ModelBackendKind::LocalRuntime,
            model_protocol: ModelProtocol::MessagesV1,
            tool_call_mode: ToolCallMode::TaggedFallback,
            model_profile: ModelProfile::default_for_backend(ModelBackendKind::LocalRuntime),
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            sandbox: SandboxConfig::default(),
            model_headers: HeaderMap::new(),
            notes_path: None,
            api: ApiConfig::default(),
            hooks: Vec::new(),
            mcp_servers: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.model_url.trim().is_empty() {
            bail!("VEX_MODEL_URL must be set");
        }
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
        if !local_endpoint && self.model_url.starts_with("http://") {
            bail!(
                "Model endpoint '{}' must use https://. Plain HTTP is allowed for local and private-network endpoints (localhost, 127.x.x.x, ::1, 0.0.0.0, and RFC 1918 LAN addresses like 192.168.x.x, 10.x.x.x, 172.16-31.x.x).",
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

    pub fn apply_interactive_model_selection(
        &mut self,
        model_url: String,
        model_name: Option<String>,
    ) {
        let previous_url = self.model_url.clone();
        let previous_backend = self.model_backend;
        let inferred_previous_backend = load::default_model_backend(&previous_url);
        let inferred_previous_protocol = load::infer_model_protocol(&previous_url);
        let inferred_previous_tool_call_mode = load::default_tool_call_mode(&previous_url);
        let preserve_backend = previous_backend != inferred_previous_backend;
        let preserve_protocol = self.model_protocol != inferred_previous_protocol;
        let preserve_tool_call_mode = self.tool_call_mode != inferred_previous_tool_call_mode;
        let next_url = model_url.trim().to_string();
        let inferred_next_backend = load::default_model_backend(&next_url);
        let next_backend = if preserve_backend {
            previous_backend
        } else {
            inferred_next_backend
        };
        let backend_changed = previous_backend != next_backend;

        self.model_url = next_url;
        if let Some(model_name) = model_name.map(|value| value.trim().to_string()) {
            if !model_name.is_empty() {
                self.model_name = model_name;
            }
        }
        self.model_backend = next_backend;
        self.model_protocol = if preserve_protocol {
            self.model_protocol
        } else {
            load::infer_model_protocol(&self.model_url)
        };
        self.tool_call_mode = if preserve_tool_call_mode {
            self.tool_call_mode
        } else {
            load::default_tool_call_mode(&self.model_url)
        };
        if backend_changed {
            self.model_profile = ModelProfile::default_for_backend(self.model_backend);
        }
    }

    pub fn should_warn_about_model_tls_skip_check(&self) -> bool {
        self.model_url_skip_tls_check && self.model_url.starts_with("https://")
    }
}

pub fn doctor_snapshot(cwd: &Path) -> Result<DoctorConfigSnapshot> {
    load::doctor_snapshot(cwd)
}

pub fn migrate_config_from_env(envs: &[(&str, &str)]) -> String {
    load::migrate_config_from_env(envs)
}
