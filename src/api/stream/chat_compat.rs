use crate::types::{ApiUsage, StreamPromptProgress, StreamTimings};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub(crate) enum ChatCompatPayload {
    Done,
    Chunk(Box<ChatCompatChunk>),
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ChatCompatChunk {
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(default)]
    pub(crate) object: Option<String>,
    #[serde(default)]
    pub(crate) created: Option<u64>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) system_fingerprint: Option<String>,
    #[serde(default)]
    pub(crate) service_tier: Option<String>,
    #[serde(default)]
    pub(crate) prompt_progress: Option<StreamPromptProgress>,
    #[serde(default)]
    pub(crate) timings: Option<StreamTimings>,
    #[serde(default)]
    pub(crate) choices: Vec<ChatCompatChoice>,
    #[serde(default)]
    pub(crate) usage: Option<ApiUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ChatCompatChoice {
    #[serde(default)]
    pub(crate) index: Option<usize>,
    #[serde(default)]
    pub(crate) delta: ChatCompatDelta,
    #[serde(default)]
    pub(crate) finish_reason: Option<String>,
    #[serde(default)]
    pub(crate) logprobs: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ChatCompatDelta {
    #[serde(default)]
    pub(crate) role: Option<String>,
    #[serde(default)]
    pub(crate) content: Option<String>,
    
    
    #[serde(default, alias = "thinking")]
    pub(crate) reasoning_content: Option<String>,
    #[serde(default)]
    pub(crate) refusal: Option<String>,
    #[serde(default)]
    pub(crate) tool_calls: Option<Vec<ChatCompatToolCallDelta>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ChatCompatToolCallDelta {
    #[serde(default)]
    pub(crate) index: Option<usize>,
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(rename = "type", default)]
    pub(crate) call_type: Option<String>,
    #[serde(default)]
    pub(crate) function: Option<ChatCompatFunctionDelta>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ChatCompatFunctionDelta {
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) arguments: Option<ChatCompatFunctionArguments>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum ChatCompatFunctionArguments {
    String(String),
    Json(serde_json::Value),
}

pub(super) fn parse_chat_compat_chunk(json_data: &str) -> Option<ChatCompatPayload> {
    if json_data == "[DONE]" {
        return Some(ChatCompatPayload::Done);
    }

    serde_json::from_str::<ChatCompatChunk>(json_data)
        .ok()
        .map(Box::new)
        .map(ChatCompatPayload::Chunk)
}
