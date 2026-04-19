use super::{LegacyStreamPayload, StreamParser, StreamProtocolMode};
use crate::runtime::json_handoff::RuntimeEnvelopeNormalizer;
use crate::runtime::{RuntimeEnvelope, RuntimeEvent};
use crate::types::{ApiUsage, StreamChunkMetadata};
use serde::Deserialize;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_STREAM_TASK_ID: AtomicU64 = AtomicU64::new(1);

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

impl StreamParser {
    pub fn finish(&mut self) -> Vec<RuntimeEnvelope> {
        if self.protocol_mode != StreamProtocolMode::ProviderNormalized {
            return Vec::new();
        }

        self.provider_normalizer_mut().finish_legacy_stream_turn()
    }

    pub(super) fn normalize_legacy_stream_payload(
        &mut self,
        payload: LegacyStreamPayload,
    ) -> Vec<RuntimeEnvelope> {
        if self.protocol_mode == StreamProtocolMode::RuntimeEnvelope {
            return vec![self.provider_error_envelope(
                "mixed_sse_protocol",
                "received legacy stream events after RuntimeEnvelope passthrough began".to_string(),
            )];
        }
        self.protocol_mode = StreamProtocolMode::ProviderNormalized;

        match payload {
            LegacyStreamPayload::Provider(event) => self
                .provider_normalizer_mut()
                .normalize_provider_stream_event(*event),
            LegacyStreamPayload::ChatCompat(payload) => self
                .provider_normalizer_mut()
                .normalize_chat_compat_payload(payload),
        }
    }

    fn provider_normalizer_mut(&mut self) -> &mut RuntimeEnvelopeNormalizer {
        self.provider_normalizer
            .get_or_insert_with(|| RuntimeEnvelopeNormalizer::new(next_stream_task_id()))
    }

    pub(super) fn provider_error_envelope(
        &mut self,
        code: &str,
        message: String,
    ) -> RuntimeEnvelope {
        if self.protocol_mode == StreamProtocolMode::RuntimeEnvelope {
            let mut normalizer = RuntimeEnvelopeNormalizer::new(next_stream_task_id());
            let _ = normalizer.start_turn(1, None);
            return normalizer.emit_event(RuntimeEvent::Error {
                code: code.to_string(),
                message,
                recoverable: true,
            });
        }

        self.protocol_mode = StreamProtocolMode::ProviderNormalized;
        self.provider_normalizer_mut()
            .emit_legacy_stream_error(code.to_string(), message)
    }
}

fn next_stream_task_id() -> String {
    let id = NEXT_STREAM_TASK_ID.fetch_add(1, Ordering::Relaxed);
    format!("api_stream_{id}")
}
