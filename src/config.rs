use anyhow::{Result, bail};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::runtime::{ModelBackendKind, ModelProtocol, SandboxConfig, ToolCallMode, ToolPolicy};
use crate::types::ModelProfile;
use crate::util::is_local_endpoint_url;

pub mod hooks;
pub use hooks::{HookConfig, HookEvent, HookOnFail, HttpHookConfig, default_hook_on_fail};

mod cache;
mod load;
mod startup;
pub use startup::StartupBudget;
#[cfg(test)]
mod tests;

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
pub struct CompactionConfig {
    pub enabled: bool,

    pub threshold_percent: u8,

    pub keep_recent_turns: usize,

    pub summary_max_tokens: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_percent: 80,
            keep_recent_turns: 4,
            summary_max_tokens: 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoMemoryConfig {
    pub enabled: bool,

    pub max_notes_per_turn: usize,
}

impl Default for AutoMemoryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_notes_per_turn: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoConfig {
    pub enabled: bool,

    pub max_checkpoints: usize,
}

impl Default for UndoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_checkpoints: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    pub enabled: bool,

    pub auto_index: bool,

    pub exclude: Vec<String>,

    pub max_file_size: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_index: true,
            exclude: vec![
                "target/".to_string(),
                "node_modules/".to_string(),
                ".git/".to_string(),
            ],
            max_file_size: 1_048_576,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchConfigLayer {
    pub(crate) enabled: Option<bool>,
    pub(crate) auto_index: Option<bool>,
    pub(crate) exclude: Option<Vec<String>>,
    pub(crate) max_file_size: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutoMemoryConfigLayer {
    pub(crate) enabled: Option<bool>,
    pub(crate) max_notes_per_turn: Option<usize>,
}

fn parse_api_client_protocol_override(value: &str) -> Option<ModelProtocol> {
    match value.trim().to_ascii_lowercase().as_str() {
        "messages-v1" => Some(ModelProtocol::MessagesV1),
        "chat-compat" => Some(ModelProtocol::ChatCompat),
        _ => None,
    }
}

fn serialize_api_client_protocol_override<S>(
    value: &Option<ModelProtocol>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(ModelProtocol::MessagesV1) => serializer.serialize_some("messages-v1"),
        Some(ModelProtocol::ChatCompat) => serializer.serialize_some("chat-compat"),
        None => serializer.serialize_none(),
    }
}

fn deserialize_api_client_protocol_override<'de, D>(
    deserializer: D,
) -> Result<Option<ModelProtocol>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let Some(value) = Option::<String>::deserialize(deserializer)? else {
        return Ok(None);
    };

    parse_api_client_protocol_override(&value)
        .map(Some)
        .ok_or_else(|| {
            serde::de::Error::custom(
                "invalid api_client.explicit_protocol: expected messages-v1 or chat-compat",
            )
        })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiClientConfig {
    pub base_url: String,

    #[serde(
        default,
        deserialize_with = "deserialize_api_client_protocol_override",
        serialize_with = "serialize_api_client_protocol_override",
        skip_serializing_if = "Option::is_none"
    )]
    pub explicit_protocol: Option<ModelProtocol>,

    pub probe_timeout_ms: u64,

    pub delta_accumulator_memory_watermark_mb: usize,
}

impl Default for ApiClientConfig {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            explicit_protocol: None,
            probe_timeout_ms: 2000,
            delta_accumulator_memory_watermark_mb: 256,
        }
    }
}

impl ApiClientConfig {
    pub fn delta_accumulator_memory_watermark_bytes(&self) -> usize {
        self.delta_accumulator_memory_watermark_mb
            .saturating_mul(1024 * 1024)
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
    pub tool_policy: ToolPolicy,
    pub model_profile: ModelProfile,

    pub max_project_instructions_tokens: usize,

    pub max_memory_tokens: usize,
    pub sandbox: SandboxConfig,
    #[serde(skip)]
    pub model_headers: HeaderMap,
    pub notes_path: Option<PathBuf>,
    pub api: ApiConfig,
    #[serde(default)]
    pub hooks: Vec<HookConfig>,
    #[serde(default)]
    pub http_hooks: Vec<HttpHookConfig>,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    #[serde(default)]
    pub compaction: CompactionConfig,
    pub undo: UndoConfig,
    pub search: SearchConfig,
    pub auto_memory: AutoMemoryConfig,

    #[serde(default)]
    pub api_client: ApiClientConfig,

    #[serde(skip)]
    pub force: bool,

    #[serde(skip)]
    pub bypass_policy: bool,

    #[serde(skip)]
    pub expand_context: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorConfigRollup {
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
    http_hooks: Option<Vec<HttpHookConfig>>,
    mcp_servers: Option<Vec<McpServerConfig>>,
    compaction: Option<CompactionConfigLayer>,
    undo: Option<UndoConfigLayer>,
    search: Option<SearchConfigLayer>,
    auto_memory: Option<AutoMemoryConfigLayer>,
    api_client: Option<ApiClientConfig>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct CompactionConfigLayer {
    enabled: Option<bool>,
    threshold_percent: Option<u8>,
    keep_recent_turns: Option<usize>,
    summary_max_tokens: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct UndoConfigLayer {
    enabled: Option<bool>,
    max_checkpoints: Option<usize>,
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
    pub fn load() -> Result<Self> {
        load::load()
    }

    pub(crate) fn load_from_cwd(cwd: &Path) -> Result<Self> {
        load::load_from_cwd(cwd)
    }

    pub fn load_cached() -> Result<Self> {
        cache::load_cached()
    }

    pub fn load_for_tests(cwd: &Path, user: Option<&Path>, system: Option<&Path>) -> Result<Self> {
        load::load_for_tests(cwd, user, system)
    }

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
            tool_call_mode: ToolCallMode::Structured,
            tool_policy: ToolPolicy::Full,
            model_profile: ModelProfile::default_for_backend(ModelBackendKind::LocalRuntime),
            max_project_instructions_tokens: 4096,
            max_memory_tokens: 2048,
            sandbox: SandboxConfig::default(),
            model_headers: HeaderMap::new(),
            notes_path: None,
            api: ApiConfig::default(),
            hooks: Vec::new(),
            http_hooks: Vec::new(),
            mcp_servers: Vec::new(),
            compaction: CompactionConfig::default(),
            undo: UndoConfig::default(),
            search: SearchConfig::default(),
            auto_memory: AutoMemoryConfig::default(),
            api_client: ApiClientConfig::default(),
            force: false,
            bypass_policy: false,
            expand_context: false,
        }
    }

    pub fn validate(&self) -> Result<()> {
        let endpoint_url = self.effective_model_endpoint_url();
        if endpoint_url.is_empty() {
            bail!("VEX_MODEL_URL or api_client.base_url must be set");
        }
        if !endpoint_url.starts_with("http://") && !endpoint_url.starts_with("https://") {
            bail!(
                "Invalid VEX_MODEL_URL '{}': expected http:// or https:// URL",
                endpoint_url
            );
        }
        if self.model_name.trim().is_empty() {
            bail!("VEX_MODEL_NAME must not be empty");
        }
        let local_endpoint = self.is_local_endpoint();
        if !local_endpoint && self.model_token.is_none() {
            bail!(
                "VEX_MODEL_TOKEN must be set for non-local endpoints (url: '{}')",
                endpoint_url
            );
        }
        if !local_endpoint && endpoint_url.starts_with("http://") {
            bail!(
                "Model endpoint '{}' must use https://. Plain HTTP is allowed for local and private-network endpoints (localhost, 127.x.x.x, ::1, 0.0.0.0, and RFC 1918 LAN addresses like 192.168.x.x, 10.x.x.x, 172.16-31.x.x).",
                endpoint_url
            );
        }
        if !local_endpoint && self.model_name.starts_with("local/") {
            bail!("Local models are only allowed for localhost endpoints");
        }
        Ok(())
    }

    fn is_local_endpoint(&self) -> bool {
        is_local_endpoint_url(self.effective_model_endpoint_url())
    }

    fn effective_model_endpoint_url(&self) -> &str {
        if self.model_url.trim().is_empty() {
            self.api_client.base_url.trim()
        } else {
            self.model_url.trim()
        }
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
        if let Some(model_name) = model_name.map(|value| value.trim().to_string())
            && !model_name.is_empty()
        {
            self.model_name = model_name;
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

pub fn doctor_rollup(cwd: &Path) -> Result<DoctorConfigRollup> {
    load::doctor_rollup(cwd)
}
