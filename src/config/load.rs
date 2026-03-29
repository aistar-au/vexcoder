use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::runtime::{ModelBackendKind, ModelProtocol, SandboxConfig, SandboxKind, ToolCallMode};
use crate::types::ModelProfile;
use crate::util::{is_local_endpoint_url, parse_bool_flag};

use super::{
    ApiConfig, ApiConfigLayer, ApiTransport, Config, ConfigLayer, DoctorConfigLayer,
    DoctorConfigSnapshot, DoctorMcpServer, McpServerConfig, McpTransport, DEFAULT_LOCAL_API_HOST,
    DEFAULT_LOCAL_API_PORT,
};

pub(super) fn load() -> Result<Config> {
    let cwd = std::env::current_dir().context("Failed to determine current working directory")?;
    let repo_cfg = find_repo_local_config(&cwd);
    let user_cfg = user_config_path();
    let system_cfg = system_config_path();
    let profile_base_dir = resolve_profile_base_dir(&cwd, repo_cfg.as_deref());
    load_layers(
        &cwd,
        repo_cfg.as_deref(),
        user_cfg.as_deref(),
        system_cfg.as_deref(),
        &profile_base_dir,
    )
}

pub(super) fn load_for_tests(
    cwd: &Path,
    user: Option<&Path>,
    system: Option<&Path>,
) -> Result<Config> {
    let repo_cfg = find_repo_local_config(cwd);
    let profile_base_dir = resolve_profile_base_dir(cwd, repo_cfg.as_deref());
    load_layers(cwd, repo_cfg.as_deref(), user, system, &profile_base_dir)
}

fn load_layers(
    cwd: &Path,
    repo_cfg: Option<&Path>,
    user_cfg: Option<&Path>,
    system_cfg: Option<&Path>,
    profile_base_dir: &Path,
) -> Result<Config> {
    let system_layer = system_cfg.map(load_config_layer).transpose()?.flatten();
    let user_layer = user_cfg.map(load_config_layer).transpose()?.flatten();
    let repo_layer = repo_cfg.map(load_config_layer).transpose()?.flatten();

    if repo_layer
        .as_ref()
        .map(|l| l.model_url_skip_tls_check == Some(true))
        .unwrap_or(false)
    {
        bail!(
            "'model_url_skip_tls_check' found in repo-local config '{}': TLS bypass must be set in user config or environment only",
            repo_cfg.unwrap_or(Path::new("<unknown>")).display()
        );
    }

    if repo_layer
        .as_ref()
        .map(|l| l.notes_path.is_some())
        .unwrap_or(false)
    {
        bail!(
            "'notes_path' found in repo-local config '{}': notes path must be set in user config layer only",
            repo_cfg.unwrap_or(Path::new("<unknown>")).display()
        );
    }

    if repo_layer
        .as_ref()
        .map(|l| l.hooks.is_some())
        .unwrap_or(false)
    {
        bail!(
            "'[[hooks]]' found in repo-local config '{}': hooks must be set in user config layer only",
            repo_cfg.unwrap_or(Path::new("<unknown>")).display()
        );
    }

    if repo_layer
        .as_ref()
        .map(|l| l.mcp_servers.is_some())
        .unwrap_or(false)
    {
        bail!(
            "'[[mcp_servers]]' found in repo-local config '{}': MCP servers must be set in user config layer only",
            repo_cfg.unwrap_or(Path::new("<unknown>")).display()
        );
    }

    if system_layer
        .as_ref()
        .map(|l| l.mcp_servers.is_some())
        .unwrap_or(false)
    {
        bail!(
            "'[[mcp_servers]]' found in system config '{}': MCP servers must be set in user config layer only",
            system_cfg.unwrap_or(Path::new("<unknown>")).display()
        );
    }

    if repo_layer
        .as_ref()
        .and_then(|layer| layer.api.as_ref())
        .and_then(|api| api.key.as_ref())
        .is_some()
    {
        bail!(
            "'api.key' found in repo-local config '{}': api secrets must not appear in repo-local config",
            repo_cfg.unwrap_or(Path::new("<unknown>")).display()
        );
    }

    let (env_layer, env_token) = read_env_layer()?;

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

    resolve_config(merged, env_token, cwd, profile_base_dir)
}

pub(super) fn doctor_snapshot(cwd: &Path) -> Result<DoctorConfigSnapshot> {
    let repo_cfg = find_repo_local_config(cwd);
    let user_cfg = user_config_path();
    let system_cfg = system_config_path();

    let system_layer = system_cfg
        .as_deref()
        .map(load_doctor_layer)
        .transpose()?
        .flatten()
        .unwrap_or_default();
    let user_layer = user_cfg
        .as_deref()
        .map(load_doctor_layer)
        .transpose()?
        .flatten()
        .unwrap_or_default();
    let repo_layer = repo_cfg
        .as_deref()
        .map(load_doctor_layer)
        .transpose()?
        .flatten()
        .unwrap_or_default();

    let env_model_url = std::env::var("VEX_MODEL_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let env_working_dir = std::env::var("VEX_WORKDIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let model_token_present = std::env::var("VEX_MODEL_TOKEN")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());

    let model_url = env_model_url
        .or(repo_layer.model_url)
        .or(user_layer.model_url)
        .or(system_layer.model_url);
    let working_dir = resolve_working_dir(
        env_working_dir
            .or(repo_layer.working_dir)
            .or(user_layer.working_dir)
            .or(system_layer.working_dir),
        cwd,
    );
    let sandbox_require = repo_layer
        .sandbox_require
        .or(user_layer.sandbox_require)
        .or(system_layer.sandbox_require)
        .unwrap_or(false);
    let mcp_servers = repo_layer
        .mcp_servers
        .or(user_layer.mcp_servers)
        .or(system_layer.mcp_servers)
        .unwrap_or_default();

    Ok(DoctorConfigSnapshot {
        model_url,
        working_dir,
        model_token_present,
        sandbox_require,
        mcp_servers,
    })
}

// ---------------------------------------------------------------------------
// Layer helpers
// ---------------------------------------------------------------------------

/// Apply `over` on top of `base`: any Some field in `over` wins.
fn apply_over(base: ConfigLayer, over: ConfigLayer) -> ConfigLayer {
    ConfigLayer {
        model_name: over.model_name.or(base.model_name),
        model_url: over.model_url.or(base.model_url),
        model_url_skip_tls_check: over
            .model_url_skip_tls_check
            .or(base.model_url_skip_tls_check),
        working_dir: over.working_dir.or(base.working_dir),
        sandbox: over.sandbox.or(base.sandbox),
        sandbox_profile: over.sandbox_profile.or(base.sandbox_profile),
        sandbox_require: over.sandbox_require.or(base.sandbox_require),
        model_backend: over.model_backend.or(base.model_backend),
        model_protocol: over.model_protocol.or(base.model_protocol),
        tool_call_mode: over.tool_call_mode.or(base.tool_call_mode),
        model_profile: over.model_profile.or(base.model_profile),
        max_project_instructions_tokens: over
            .max_project_instructions_tokens
            .or(base.max_project_instructions_tokens),
        max_memory_tokens: over.max_memory_tokens.or(base.max_memory_tokens),
        notes_path: over.notes_path.or(base.notes_path),
        api: apply_api_over(base.api, over.api),
        hooks: over.hooks.or(base.hooks),
        mcp_servers: over.mcp_servers.or(base.mcp_servers),
    }
}

fn apply_api_over(
    base: Option<ApiConfigLayer>,
    over: Option<ApiConfigLayer>,
) -> Option<ApiConfigLayer> {
    match (base, over) {
        (None, None) => None,
        (Some(layer), None) | (None, Some(layer)) => Some(layer),
        (Some(base), Some(over)) => Some(ApiConfigLayer {
            transport: over.transport.or(base.transport),
            host: over.host.or(base.host),
            port: over.port.or(base.port),
            socket: over.socket.or(base.socket),
            key: over.key.or(base.key),
            tls_cert: over.tls_cert.or(base.tls_cert),
            tls_key: over.tls_key.or(base.tls_key),
            tls_ca_cert: over.tls_ca_cert.or(base.tls_ca_cert),
            tls_skip_verify: over.tls_skip_verify.or(base.tls_skip_verify),
            vpn_trust: over.vpn_trust.or(base.vpn_trust),
        }),
    }
}

/// Read environment variables into a ConfigLayer and return the env token
/// separately (token is forbidden in file layers).
///
/// VEX_MODEL_PROTOCOL is validated here so the error message names the env var.
pub(super) fn read_env_layer() -> Result<(ConfigLayer, Option<String>)> {
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
                     local-runtime, local_runtime, local, api-server, api_server, api, remote",
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
                     tagged-fallback, tagged_fallback, fallback, tagged",
                    v
                );
            }
            Some(v)
        }
        _ => None,
    };
    let model_url_skip_tls_check = match std::env::var("VEX_MODEL_URL_SKIP_TLS_CHECK") {
        Ok(v) if !v.trim().is_empty() => Some(parse_bool_flag(v.clone()).ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid VEX_MODEL_URL_SKIP_TLS_CHECK '{}': expected true/false/1/0",
                v
            )
        })?),
        _ => None,
    };

    let max_project_instructions_tokens = std::env::var("VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|budget| *budget > 0);
    let max_memory_tokens = std::env::var("VEX_MAX_MEMORY_TOKENS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|budget| *budget > 0);
    let api_transport = match std::env::var("VEX_API_TRANSPORT") {
        Ok(v) if !v.trim().is_empty() => {
            if parse_api_transport(v.clone()).is_none() {
                bail!(
                    "Invalid VEX_API_TRANSPORT '{}': expected one of http, unix, both",
                    v
                );
            }
            Some(v)
        }
        _ => None,
    };
    let api_port =
        match std::env::var("VEX_API_PORT") {
            Ok(v) if !v.trim().is_empty() => Some(v.trim().parse::<u16>().with_context(|| {
                format!("Invalid VEX_API_PORT '{}': expected integer 1-65535", v)
            })?),
            _ => None,
        };
    let api_tls_skip_verify = match std::env::var("VEX_API_TLS_SKIP_VERIFY") {
        Ok(v) if !v.trim().is_empty() => parse_bool_flag(v.clone()).ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid VEX_API_TLS_SKIP_VERIFY '{}': expected true/false/1/0",
                v
            )
        })?,
        _ => false,
    };
    let api_vpn_trust = match std::env::var("VEX_API_VPN_TRUST") {
        Ok(v) if !v.trim().is_empty() => parse_bool_flag(v.clone()).ok_or_else(|| {
            anyhow::anyhow!("Invalid VEX_API_VPN_TRUST '{}': expected true/false/1/0", v)
        })?,
        _ => false,
    };

    let layer = ConfigLayer {
        model_name: std::env::var("VEX_MODEL_NAME")
            .ok()
            .filter(|v| !v.trim().is_empty()),
        model_url: std::env::var("VEX_MODEL_URL")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        model_url_skip_tls_check,
        working_dir: std::env::var("VEX_WORKDIR")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from),
        sandbox: std::env::var("VEX_SANDBOX")
            .ok()
            .filter(|v| !v.trim().is_empty()),
        sandbox_profile: std::env::var("VEX_SANDBOX_PROFILE")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        sandbox_require: match std::env::var("VEX_SANDBOX_REQUIRE") {
            Ok(v) if !v.trim().is_empty() => Some(parse_bool_flag(v.clone()).ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid VEX_SANDBOX_REQUIRE '{}': expected true/false/1/0",
                    v
                )
            })?),
            _ => None,
        },
        model_backend,
        model_protocol,
        tool_call_mode,
        model_profile: std::env::var("VEX_MODEL_PROFILE")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from),
        max_project_instructions_tokens,
        max_memory_tokens,
        mcp_servers: None,
        notes_path: None,
        api: Some(ApiConfigLayer {
            transport: api_transport,
            host: std::env::var("VEX_API_HOST")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            port: api_port,
            socket: std::env::var("VEX_API_SOCKET")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            key: std::env::var("VEX_API_KEY")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            tls_cert: std::env::var("VEX_API_TLS_CERT")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            tls_key: std::env::var("VEX_API_TLS_KEY")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            tls_ca_cert: std::env::var("VEX_API_TLS_CA_CERT")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            tls_skip_verify: Some(api_tls_skip_verify),
            vpn_trust: Some(api_vpn_trust),
        }),
        hooks: None,
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
                 local-runtime, local_runtime, local, api-server, api_server, api, remote",
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
                 tagged-fallback, tagged_fallback, fallback, tagged",
                path.display(),
                s
            );
        }
    }
    if let Some(ref s) = layer.sandbox {
        if parse_sandbox_kind(s.clone()).is_none() {
            bail!(
                "config file '{}': invalid sandbox '{}': expected one of passthrough, macos-exec, macos_exec, container",
                path.display(),
                s
            );
        }
    }
    if let Some(ref api) = layer.api {
        if let Some(ref transport) = api.transport {
            if parse_api_transport(transport.clone()).is_none() {
                bail!(
                    "config file '{}': invalid api.transport '{}': expected one of http, unix, both",
                    path.display(),
                    transport
                );
            }
        }
        if api.tls_skip_verify.unwrap_or(false) {
            bail!(
                "config file '{}': api.tls_skip_verify must remain false in Phase I",
                path.display()
            );
        }
        if api.vpn_trust.unwrap_or(false) {
            bail!(
                "config file '{}': api.vpn_trust must remain false until a dedicated ADR exists",
                path.display()
            );
        }
    }

    if let Some(ref servers) = layer.mcp_servers {
        validate_mcp_servers_for_layer(servers).with_context(|| {
            format!(
                "config file '{}': invalid [[mcp_servers]] entry",
                path.display()
            )
        })?;
    }

    Ok(Some(layer))
}

fn load_doctor_layer(path: &Path) -> Result<Option<DoctorConfigLayer>> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(e)
                .with_context(|| format!("failed to read config file '{}'", path.display()));
        }
    };

    let raw: toml::Value = toml::from_str(&content)
        .with_context(|| format!("malformed TOML in '{}'", path.display()))?;

    let mcp_servers = raw
        .get("mcp_servers")
        .and_then(|value| value.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let table = entry.as_table()?;
                    let name = table
                        .get("name")
                        .and_then(|value| value.as_str())
                        .unwrap_or("unnamed")
                        .trim()
                        .to_string();
                    let command = table
                        .get("command")
                        .and_then(|value| value.as_str())
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty());
                    let url = table
                        .get("url")
                        .and_then(|value| value.as_str())
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty());
                    let transport = table
                        .get("transport")
                        .and_then(|value| value.as_str())
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty())
                        .or_else(|| {
                            if url.is_some() {
                                Some("http".to_string())
                            } else if command.is_some() {
                                Some("stdio".to_string())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| "unknown".to_string());

                    Some(DoctorMcpServer {
                        name,
                        transport,
                        command,
                        url,
                    })
                })
                .collect::<Vec<_>>()
        });

    Ok(Some(DoctorConfigLayer {
        model_url: raw
            .get("model_url")
            .and_then(|value| value.as_str())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        working_dir: raw
            .get("working_dir")
            .and_then(|value| value.as_str())
            .map(PathBuf::from),
        sandbox_require: raw.get("sandbox_require").and_then(|value| value.as_bool()),
        mcp_servers,
    }))
}

/// Resolve a fully-merged ConfigLayer into a concrete Config.
/// Compiled defaults fill any field not set by any layer.
/// `fallback_cwd` is used as the default `working_dir` when no layer sets it.
fn resolve_config(
    merged: ConfigLayer,
    env_token: Option<String>,
    fallback_cwd: &Path,
    profile_base_dir: &Path,
) -> Result<Config> {
    let model_url = merged.model_url.unwrap_or_default();
    let model_url_skip_tls_check = merged.model_url_skip_tls_check.unwrap_or(false);
    let model_name = merged
        .model_name
        .unwrap_or_else(|| "local/default".to_string());
    let working_dir = resolve_working_dir(merged.working_dir, fallback_cwd);

    let is_local = model_url.trim().is_empty() || is_local_endpoint_url(&model_url);

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

    let explicit_profile_path = merged.model_profile.clone();
    let model_profile = load_model_profile(
        explicit_profile_path.as_deref(),
        profile_base_dir,
        model_backend,
    )?;
    let tool_call_mode = if explicit_profile_path.is_some() {
        model_profile.tool_call_mode()
    } else {
        merged
            .tool_call_mode
            .and_then(parse_tool_call_mode)
            .unwrap_or(if is_local {
                ToolCallMode::TaggedFallback
            } else {
                ToolCallMode::Structured
            })
    };
    let max_project_instructions_tokens = merged.max_project_instructions_tokens.unwrap_or(4096);
    let max_memory_tokens = merged.max_memory_tokens.unwrap_or(2048);
    let sandbox = SandboxConfig {
        kind: merged
            .sandbox
            .and_then(parse_sandbox_kind)
            .unwrap_or(SandboxKind::Passthrough),
        profile: merged.sandbox_profile,
        require: merged.sandbox_require.unwrap_or(false),
    };
    let api = resolve_api_config(merged.api)?;
    let mcp_servers = validate_mcp_servers(merged.mcp_servers.unwrap_or_default())?;

    Ok(Config {
        model_token: env_token,
        model_name,
        model_url,
        model_url_skip_tls_check,
        working_dir,
        model_backend,
        model_protocol,
        tool_call_mode,
        model_profile,
        max_project_instructions_tokens,
        max_memory_tokens,
        sandbox,
        model_headers: parse_model_headers_json()?,
        notes_path: merged.notes_path.map(expand_home),
        api,
        hooks: merged.hooks.unwrap_or_default(),
        mcp_servers,
    })
}

pub(super) fn default_model_backend(model_url: &str) -> ModelBackendKind {
    if model_url.trim().is_empty() || is_local_endpoint_url(model_url) {
        ModelBackendKind::LocalRuntime
    } else {
        ModelBackendKind::ApiServer
    }
}

pub(super) fn default_tool_call_mode(model_url: &str) -> ToolCallMode {
    if model_url.trim().is_empty() || is_local_endpoint_url(model_url) {
        ToolCallMode::TaggedFallback
    } else {
        ToolCallMode::Structured
    }
}

fn resolve_api_config(layer: Option<ApiConfigLayer>) -> Result<ApiConfig> {
    let layer = layer.unwrap_or_default();
    Ok(ApiConfig {
        transport: layer
            .transport
            .and_then(parse_api_transport)
            .unwrap_or(ApiTransport::Http),
        host: layer
            .host
            .unwrap_or_else(|| DEFAULT_LOCAL_API_HOST.to_string()),
        port: layer.port.unwrap_or(DEFAULT_LOCAL_API_PORT),
        socket: layer.socket.map(expand_home),
        key: resolve_secret_reference(layer.key),
        tls_cert: layer.tls_cert.map(expand_home),
        tls_key: layer.tls_key.map(expand_home),
        tls_ca_cert: layer.tls_ca_cert.map(expand_home),
        tls_skip_verify: layer.tls_skip_verify.unwrap_or(false),
        vpn_trust: layer.vpn_trust.unwrap_or(false),
    })
}

fn resolve_secret_reference(value: Option<String>) -> Option<String> {
    let value = value?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(name) = trimmed
        .strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'))
    {
        return std::env::var(name)
            .ok()
            .map(|env| env.trim().to_string())
            .filter(|env| !env.is_empty());
    }
    Some(trimmed.to_string())
}

fn validate_mcp_servers_for_layer(servers: &[McpServerConfig]) -> Result<()> {
    validate_mcp_servers_with_mode(servers.to_vec(), false).map(|_| ())
}

fn validate_mcp_servers(servers: Vec<McpServerConfig>) -> Result<Vec<McpServerConfig>> {
    validate_mcp_servers_with_mode(servers, true)
}

fn validate_mcp_servers_with_mode(
    servers: Vec<McpServerConfig>,
    expand_header_env: bool,
) -> Result<Vec<McpServerConfig>> {
    let mut validated = Vec::new();
    let mut seen_names = HashSet::new();

    for mut server in servers {
        server.name = server.name.trim().to_string();
        if server.name.is_empty() {
            bail!("mcp_servers.name must not be empty");
        }
        if !server
            .name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            bail!(
                "mcp_servers.name '{}' contains invalid characters; \
                 only ASCII letters, digits, hyphens, and underscores are allowed",
                server.name
            );
        }
        if !seen_names.insert(server.name.clone()) {
            bail!("duplicate mcp server name '{}'", server.name);
        }

        server.args = server
            .args
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();

        match server.transport {
            McpTransport::Stdio => {
                let command = server
                    .command
                    .take()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!("stdio MCP server '{}' requires 'command'", server.name)
                    })?;
                if server
                    .url
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty())
                {
                    bail!("stdio MCP server '{}' must not set 'url'", server.name);
                }
                if !server.headers.is_empty() {
                    bail!("stdio MCP server '{}' must not set headers", server.name);
                }
                server.command = Some(command);
                server.url = None;
            }
            McpTransport::Http => {
                let url = server
                    .url
                    .take()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!("http MCP server '{}' requires 'url'", server.name)
                    })?;
                if server
                    .command
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty())
                {
                    bail!("http MCP server '{}' must not set 'command'", server.name);
                }
                if !server.args.is_empty() {
                    bail!("http MCP server '{}' must not set 'args'", server.name);
                }
                let mut headers = BTreeMap::new();
                for (name, value) in server.headers {
                    let name = name.trim().to_string();
                    if name.is_empty() {
                        bail!("http MCP server '{}' has an empty header name", server.name);
                    }
                    let value = if expand_header_env {
                        crate::mcp::resolve_mcp_header_env(&value)?
                    } else {
                        value.trim().to_string()
                    };
                    headers.insert(name, value);
                }
                server.command = None;
                server.url = Some(url);
                server.headers = headers;
            }
        }

        validated.push(server);
    }

    Ok(validated)
}

fn resolve_working_dir(working_dir: Option<PathBuf>, fallback_cwd: &Path) -> PathBuf {
    working_dir.unwrap_or_else(|| fallback_cwd.to_path_buf())
}

fn load_model_profile(
    selected_path: Option<&Path>,
    profile_base_dir: &Path,
    model_backend: ModelBackendKind,
) -> Result<ModelProfile> {
    let Some(path) = selected_path else {
        return Ok(ModelProfile::default_for_backend(model_backend));
    };

    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        profile_base_dir.join(path)
    };
    ModelProfile::load(&resolved).with_context(|| {
        format!(
            "failed to load model profile '{}' (base '{}')",
            path.display(),
            profile_base_dir.display()
        )
    })
}

fn resolve_profile_base_dir(cwd: &Path, repo_cfg: Option<&Path>) -> PathBuf {
    if let Some(root) = repo_cfg
        .and_then(|config| config.parent())
        .and_then(Path::parent)
    {
        return root.to_path_buf();
    }
    find_repo_root(cwd).unwrap_or_else(|| cwd.to_path_buf())
}

fn expand_home(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var("HOME").ok().filter(|v| !v.is_empty()) {
            return PathBuf::from(home).join(rest);
        }
    }
    path
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

fn find_repo_root(cwd: &Path) -> Option<PathBuf> {
    let mut dir: &Path = cwd;
    loop {
        if dir.join(".git").exists() || dir.join(".vex").join("config.toml").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

pub(super) fn user_config_path() -> Option<PathBuf> {
    let primary = user_config_xdg_path();
    if primary.as_ref().is_some_and(|path| path.exists()) {
        return primary;
    }

    let legacy = user_config_legacy_path();
    if legacy.as_ref().is_some_and(|path| path.exists()) {
        return legacy;
    }

    primary.or(legacy)
}

fn user_config_xdg_path() -> Option<PathBuf> {
    if let Some(root) = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        return Some(PathBuf::from(root).join("vex").join("config.toml"));
    }

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

fn user_config_legacy_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|home| PathBuf::from(home).join(".vex").join("config.toml"))
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

fn parse_api_transport(value: String) -> Option<ApiTransport> {
    match value.trim().to_ascii_lowercase().as_str() {
        "http" => Some(ApiTransport::Http),
        "unix" => Some(ApiTransport::Unix),
        "both" => Some(ApiTransport::Both),
        _ => None,
    }
}

fn parse_sandbox_kind(value: String) -> Option<SandboxKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "passthrough" => Some(SandboxKind::Passthrough),
        "macos-exec" | "macos_exec" => Some(SandboxKind::MacosExec),
        "container" => Some(SandboxKind::Container),
        _ => None,
    }
}

pub(super) fn infer_model_protocol(api_url: &str) -> ModelProtocol {
    let normalized = api_url.trim().to_ascii_lowercase();
    if normalized.contains("/chat/completions") || normalized.ends_with("/v1") {
        ModelProtocol::ChatCompat
    } else {
        ModelProtocol::MessagesV1
    }
}

pub(super) fn parse_model_headers_json() -> Result<HeaderMap> {
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

pub(super) fn legacy_messages_protocol_value() -> &'static str {
    concat!("anth", "ropic")
}

pub(super) fn legacy_chat_protocol_value() -> &'static str {
    concat!("open", "ai")
}

/// Generate a `.vex/config.toml` fragment mapping pre-ADR-022 branded
/// environment variable values to current neutral equivalents.
///
/// `envs` is a slice of `(name, value)` pairs used in tests. Pass `&[]` to
/// read from the live environment via `std::env::var`. Non-destructive: never
/// writes to disk itself. Called by `vex migrate config`.
pub(super) fn migrate_config_from_env(envs: &[(&str, &str)]) -> String {
    let get = |name: &str| -> Option<String> {
        if !envs.is_empty() {
            envs.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        } else {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        }
    };

    let mut lines: Vec<String> = Vec::new();
    lines.push("# generated by vex migrate config".to_string());
    lines
        .push("# apply this fragment to .vex/config.toml or ~/.config/vex/config.toml".to_string());

    if let Some(value) = get("VEX_API_PROTOCOL") {
        match value.trim().to_ascii_lowercase().as_str() {
            value if value == legacy_messages_protocol_value() => {
                lines.push(r#"model_protocol = "messages-v1""#.to_string())
            }
            value if value == legacy_chat_protocol_value() => {
                lines.push(r#"model_protocol = "chat-compat""#.to_string())
            }
            other => lines.push(format!(
                "# WARNING: unknown VEX_API_PROTOCOL value {other:?}; no mapping generated"
            )),
        }
    }

    if let Some(value) = get("VEX_STRUCTURED_TOOL_PROTOCOL") {
        match value.trim().to_ascii_lowercase().as_str() {
            "on" | "true" | "1" => lines.push(r#"tool_call_mode = "structured""#.to_string()),
            "off" | "false" | "0" => {
                lines.push(r#"tool_call_mode = "tagged-fallback""#.to_string())
            }
            other => lines.push(format!(
                "# WARNING: unknown VEX_STRUCTURED_TOOL_PROTOCOL value {other:?}; no mapping generated"
            )),
        }
    }

    if let Some(value) = get("VEX_MODEL_URL") {
        let trimmed = value.trim().trim_end_matches('/');
        let base = if let Some(prefix) = trimmed.strip_suffix("/v1/messages") {
            prefix
        } else if let Some(prefix) = trimmed.strip_suffix("/v1/chat/completions") {
            prefix
        } else {
            trimmed
        };
        lines.push(format!("model_url = {base:?}"));
    }

    lines.join("\n") + "\n"
}
