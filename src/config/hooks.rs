use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    PreTool,
    PostTool,
}


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


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpHookConfig {
    pub event: HookEvent,
    pub tool: String,
    pub url: String,
    #[serde(default = "default_hook_on_fail")]
    pub on_fail: HookOnFail,
}

impl HttpHookConfig {
    
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
