use crate::types::{ApiUsage, StreamChunkMetadata};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ProviderStreamEvent {
    MessageStart {
        message: ProviderMessageStartData,
    },
    ContentBlockStart {
        index: usize,
        content_block: serde_json::Value,
    },
    ContentBlockDelta {
        index: usize,
        delta: ProviderDelta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: ProviderMessageDelta,
        #[serde(default)]
        usage: Option<ApiUsage>,
    },
    MessageStop,
    Ping,
    Error {
        error: ProviderApiStreamError,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ProviderMessageStartData {
    #[serde(rename = "id")]
    pub(crate) _id: String,
    #[serde(rename = "type", default)]
    pub(crate) _message_type: Option<String>,
    #[serde(rename = "role")]
    pub(crate) _role: String,
    #[serde(rename = "model")]
    pub(crate) _model: String,
    #[serde(default)]
    pub(crate) _content: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub(crate) _stop_reason: Option<String>,
    #[serde(default)]
    pub(crate) _stop_sequence: Option<String>,
    #[serde(default)]
    pub(crate) usage: Option<ApiUsage>,
    #[serde(default)]
    pub(crate) metadata: Option<StreamChunkMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ProviderMessageDelta {
    #[serde(rename = "stop_reason")]
    pub(crate) _stop_reason: Option<String>,
    #[serde(default)]
    pub(crate) _stop_sequence: Option<String>,
    #[serde(default)]
    pub(crate) _role: Option<String>,
    #[serde(default)]
    pub(crate) _refusal: Option<String>,
    #[serde(default)]
    pub(crate) metadata: Option<StreamChunkMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ProviderApiStreamError {
    #[serde(rename = "type")]
    pub(crate) error_type: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ProviderDelta {
    #[serde(rename = "type")]
    #[serde(default)]
    pub(crate) _delta_type: Option<String>,
    #[serde(default)]
    pub(crate) text: Option<String>,
    #[serde(default)]
    pub(crate) partial_json: Option<String>,
    #[serde(default)]
    pub(crate) thinking: Option<String>,
    #[serde(default)]
    pub(crate) _signature: Option<String>,
    #[serde(default)]
    pub(crate) _choice_index: Option<usize>,
}
