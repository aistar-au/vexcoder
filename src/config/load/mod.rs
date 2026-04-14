mod merge;
mod parse;
mod paths;

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use crate::util::parse_bool_flag;

use super::{
    ApiConfigLayer, AutoMemoryConfigLayer, CompactionConfigLayer, Config, ConfigLayer,
    DoctorConfigRollup, SearchConfigLayer, UndoConfigLayer,
};

use merge::apply_over;
use parse::*;
use paths::*;

// Re-exports for parent module (config.rs test-only imports)
pub(super) use parse::infer_model_protocol;
#[cfg(test)]
pub(super) use parse::{
    legacy_chat_protocol_value, legacy_messages_protocol_value, parse_model_headers_json,
};
pub(super) use paths::user_config_path;

pub(super) fn load() -> Result<Config> {
    let cwd = std::env::current_dir().context("Failed to determine current working directory")?;
    load_from_cwd(&cwd)
}

pub(super) fn load_from_cwd(cwd: &Path) -> Result<Config> {
    let repo_cfg = find_repo_local_config(cwd);
    let user_cfg = user_config_path();
    let system_cfg = system_config_path();
    let profile_base_dir = resolve_profile_base_dir(cwd, repo_cfg.as_deref());
    load_layers(
        cwd,
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
        .map(|l| l.http_hooks.is_some())
        .unwrap_or(false)
    {
        bail!(
            "'[[http_hooks]]' found in repo-local config '{}': http hooks must be set in user config layer only",
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

pub(super) fn doctor_rollup(cwd: &Path) -> Result<DoctorConfigRollup> {
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
    let env_sandbox_require = match std::env::var("VEX_SANDBOX_REQUIRE") {
        Ok(value) if !value.trim().is_empty() => {
            Some(parse_bool_flag(value.clone()).ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid VEX_SANDBOX_REQUIRE '{}': expected true/false/1/0",
                    value
                )
            })?)
        }
        _ => None,
    };
    let model_token_present = model_token_from_env_or_keyring().is_some();

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
    let sandbox_require = env_sandbox_require
        .or(repo_layer.sandbox_require)
        .or(user_layer.sandbox_require)
        .or(system_layer.sandbox_require)
        .unwrap_or(false);
    let mcp_servers = repo_layer
        .mcp_servers
        .or(user_layer.mcp_servers)
        .or(system_layer.mcp_servers)
        .unwrap_or_default();

    Ok(DoctorConfigRollup {
        model_url,
        working_dir,
        model_token_present,
        sandbox_require,
        mcp_servers,
    })
}

fn model_token_from_env_or_keyring() -> Option<String> {
    model_token_from_env_or_keyring_with(crate::credentials::read)
}

#[cfg(test)]
pub(super) fn model_token_from_env_or_keyring_with<F>(keyring_read: F) -> Option<String>
where
    F: FnOnce(&str) -> Result<Option<String>>,
{
    model_token_from_env_or_keyring_with_impl(keyring_read)
}

#[cfg(not(test))]
fn model_token_from_env_or_keyring_with<F>(keyring_read: F) -> Option<String>
where
    F: FnOnce(&str) -> Result<Option<String>>,
{
    model_token_from_env_or_keyring_with_impl(keyring_read)
}

fn model_token_from_env_or_keyring_with_impl<F>(keyring_read: F) -> Option<String>
where
    F: FnOnce(&str) -> Result<Option<String>>,
{
    std::env::var("VEX_MODEL_TOKEN")
        .ok()
        .and_then(|value| {
            if value.trim().is_empty() {
                None
            } else {
                Some(value)
            }
        })
        .or_else(
            || match keyring_read(crate::credentials::ACCOUNT_MODEL_TOKEN) {
                Ok(value) => value,
                Err(err) => {
                    tracing::debug!(error = %err, "keyring read failed; token remains absent");
                    None
                }
            },
        )
}

/// Read environment variables into a ConfigLayer and return the env token
/// separately (token is forbidden in file layers).
///
/// VEX_MODEL_PROTOCOL is validated here so the error message names the env var.
pub(super) fn read_env_layer() -> Result<(ConfigLayer, Option<String>)> {
    let env_token = model_token_from_env_or_keyring();

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
        compaction: None,
        undo: None,
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
        http_hooks: None,
        search: None,
        auto_memory: None,
    };

    Ok((layer, env_token))
}

/// Load and validate a single TOML config file.
///
/// Returns `Ok(None)` when the file does not exist (not an error).
/// Returns `Err` for: `model_token` present, unknown keys, malformed TOML,
/// or invalid enum string values ΓÇö all with the file path in the message.
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
                "config file '{}': invalid sandbox '{}': expected one of passthrough, macos-exec, macos_exec, container, bubblewrap, bwrap, linux-bwrap",
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

    if let Some(ref http_hooks) = layer.http_hooks {
        for hook in http_hooks {
            hook.validate().with_context(|| {
                format!(
                    "config file '{}': invalid [[http_hooks]] entry",
                    path.display()
                )
            })?;
        }
    }

    Ok(Some(layer))
}

mod resolve;
use resolve::*;

pub(super) use resolve::{default_model_backend, default_tool_call_mode, migrate_config_from_env};

#[cfg(test)]
mod tests;
