use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// Which edge of a tool invocation the hook fires on.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    PreTool,
    PostTool,
}

/// What to do when the hook command exits with a non-zero status.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookOnFail {
    Warn,
    Abort,
    Ignore,
}

pub fn default_hook_on_fail() -> HookOnFail {
    HookOnFail::Warn
}

/// One `[[hooks]]` entry from the user config layer.
///
/// Repo-local `.vex/config.toml` must never contain `[[hooks]]` — that is
/// enforced at config load time (same supply-chain rationale as
/// `[[mcp_servers]]`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookConfig {
    pub event: HookEvent,
    pub tool: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_hook_on_fail")]
    pub on_fail: HookOnFail,
}

/// One `[[http_hooks]]` entry from the user config layer.
///
/// Fires a POST request to `url` on every matching tool event, delivering a
/// JSON payload with the event name, tool name, primary path, and timestamp.
///
/// Repo-local `.vex/config.toml` must never contain `[[http_hooks]]` — same
/// supply-chain rationale as `[[hooks]]` and `[[mcp_servers]]`.
///
/// The `url` must start with `https://` or `http://`.  Plain-text `http://`
/// is accepted but callers should prefer `https://` for production webhooks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpHookConfig {
    pub event: HookEvent,
    pub tool: String,
    pub url: String,
    #[serde(default = "default_hook_on_fail")]
    pub on_fail: HookOnFail,
}

impl HttpHookConfig {
    /// Validate that the `url` field is a well-formed HTTP/HTTPS URL.
    pub fn validate(&self) -> Result<()> {
        if !self.url.starts_with("https://") && !self.url.starts_with("http://") {
            bail!(
                "http_hook url must start with 'https://' or 'http://', got: {}",
                self.url
            );
        }
        Ok(())
    }
}
