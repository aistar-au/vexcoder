use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::runtime::{ModelBackendKind, SandboxConfig, SandboxKind, ToolCallMode, ToolPolicy};
use crate::util::is_local_endpoint_url;

use super::super::{
    ApiConfig, ApiConfigLayer, ApiTransport, CompactionConfig, CompactionConfigLayer, Config,
    ConfigLayer, DEFAULT_LOCAL_API_HOST, DEFAULT_LOCAL_API_PORT, DoctorConfigLayer,
    DoctorMcpServer, HttpHookConfig, McpServerConfig, McpTransport, SearchConfig,
    SearchConfigLayer, UndoConfig, UndoConfigLayer,
};

use super::merge::resolve_auto_memory_config;
use super::parse::*;
use super::paths::*;

pub(super) fn load_doctor_layer(path: &Path) -> Result<Option<DoctorConfigLayer>> {
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
pub(super) fn resolve_config(
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
        tool_policy: ToolPolicy::Full,
        model_profile,
        max_project_instructions_tokens,
        max_memory_tokens,
        sandbox,
        model_headers: parse_model_headers_json()?,
        notes_path: merged.notes_path.map(expand_home),
        api,
        hooks: merged.hooks.unwrap_or_default(),
        http_hooks: validate_http_hooks(merged.http_hooks.unwrap_or_default())?,
        mcp_servers,
        compaction: resolve_compaction_config(merged.compaction),
        undo: resolve_undo_config(merged.undo),
        search: resolve_search_config(merged.search),
        auto_memory: resolve_auto_memory_config(merged.auto_memory),
    })
}

pub(super) fn resolve_compaction_config(layer: Option<CompactionConfigLayer>) -> CompactionConfig {
    let layer = layer.unwrap_or_default();
    CompactionConfig {
        enabled: layer.enabled.unwrap_or(false),
        threshold_percent: layer.threshold_percent.unwrap_or(80).clamp(10, 99),
        keep_recent_turns: layer.keep_recent_turns.unwrap_or(4).clamp(1, 32),
        summary_max_tokens: layer.summary_max_tokens.unwrap_or(1024).clamp(64, 4096),
    }
}

pub(super) fn resolve_undo_config(layer: Option<UndoConfigLayer>) -> UndoConfig {
    let defaults = UndoConfig::default();
    match layer {
        None => defaults,
        Some(l) => UndoConfig {
            enabled: l.enabled.unwrap_or(defaults.enabled),
            max_checkpoints: l
                .max_checkpoints
                .unwrap_or(defaults.max_checkpoints)
                .clamp(1, 100),
        },
    }
}

pub(crate) fn default_model_backend(model_url: &str) -> ModelBackendKind {
    if model_url.trim().is_empty() || is_local_endpoint_url(model_url) {
        ModelBackendKind::LocalRuntime
    } else {
        ModelBackendKind::ApiServer
    }
}

pub(crate) fn default_tool_call_mode(model_url: &str) -> ToolCallMode {
    if model_url.trim().is_empty() || is_local_endpoint_url(model_url) {
        ToolCallMode::TaggedFallback
    } else {
        ToolCallMode::Structured
    }
}

pub(super) fn resolve_api_config(layer: Option<ApiConfigLayer>) -> Result<ApiConfig> {
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

pub(super) fn resolve_search_config(layer: Option<SearchConfigLayer>) -> SearchConfig {
    let defaults = SearchConfig::default();
    match layer {
        None => defaults,
        Some(l) => {
            let exclude = normalize_exclude_prefixes(l.exclude.unwrap_or(defaults.exclude));
            SearchConfig {
                enabled: l.enabled.unwrap_or(defaults.enabled),
                auto_index: l.auto_index.unwrap_or(defaults.auto_index),
                exclude,
                max_file_size: l.max_file_size.unwrap_or(defaults.max_file_size),
            }
        }
    }
}

/// Ensure every exclude entry ends with `/` so `starts_with` prefix matching
/// in the index cannot false-positive on paths that merely share a common
/// stem (e.g. `"src"` must not match `"src_utils/foo.rs"`).
pub(super) fn normalize_exclude_prefixes(entries: Vec<String>) -> Vec<String> {
    entries
        .into_iter()
        .map(|mut entry| {
            if !entry.ends_with('/') {
                entry.push('/');
            }
            entry
        })
        .collect()
}

pub(super) fn resolve_secret_reference(value: Option<String>) -> Option<String> {
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

pub(super) fn validate_mcp_servers_for_layer(servers: &[McpServerConfig]) -> Result<()> {
    validate_mcp_servers_with_mode(servers.to_vec(), false).map(|_| ())
}

pub(super) fn validate_mcp_servers(servers: Vec<McpServerConfig>) -> Result<Vec<McpServerConfig>> {
    validate_mcp_servers_with_mode(servers, true)
}

pub(super) fn validate_http_hooks(hooks: Vec<HttpHookConfig>) -> Result<Vec<HttpHookConfig>> {
    for hook in &hooks {
        hook.validate()?;
    }
    Ok(hooks)
}

pub(super) fn validate_mcp_servers_with_mode(
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

/// Generate a `.vex/config.toml` fragment mapping pre-ADR-022 branded
/// environment variable values to current neutral equivalents.
///
/// `envs` is a slice of `(name, value)` pairs used in tests. Pass `&[]` to
/// read from the process environment via `std::env::var`. Non-destructive: never
/// writes to disk itself. Called by `vex migrate config`.
pub(crate) fn migrate_config_from_env(envs: &[(&str, &str)]) -> String {
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
