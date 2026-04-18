use super::{
    StreamParser, StreamProtocolMode, THINKING_DATA_TAG, legacy_external_thinking_tag,
    transitional_internal_thinking_tag,
};
use crate::runtime::json_handoff::{RuntimeEnvelopeNormalizer, TurnEndContext};
use crate::runtime::{RuntimeEnvelope, RuntimeEvent, TokenUsageEnvelope, UiUpdate};
use crate::state::{StreamBlock, ToolStatus};
use crate::types::{ApiUsage, StreamChunkMetadata};
use crate::usage::TurnTokens;
use serde::Deserialize;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_STREAM_TASK_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ProviderStreamEvent {
    MessageStart {
        message: ProviderMessageStartData,
    },
    ContentBlockStart {
        index: usize,
        content_block: ProviderContentBlock,
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
pub(super) struct ProviderMessageStartData {
    #[serde(rename = "id")]
    pub(super) _id: String,
    #[serde(rename = "type", default)]
    pub(super) _message_type: Option<String>,
    #[serde(rename = "role")]
    pub(super) _role: String,
    #[serde(rename = "model")]
    pub(super) _model: String,
    #[serde(default)]
    pub(super) _content: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub(super) _stop_reason: Option<String>,
    #[serde(default)]
    pub(super) _stop_sequence: Option<String>,
    #[serde(default)]
    pub(super) usage: Option<ApiUsage>,
    #[serde(default)]
    pub(super) metadata: Option<StreamChunkMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ProviderMessageDelta {
    #[serde(rename = "stop_reason")]
    pub(super) _stop_reason: Option<String>,
    #[serde(default)]
    pub(super) _stop_sequence: Option<String>,
    #[serde(default)]
    pub(super) _role: Option<String>,
    #[serde(default)]
    pub(super) _refusal: Option<String>,
    #[serde(default)]
    pub(super) metadata: Option<StreamChunkMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ProviderApiStreamError {
    #[serde(rename = "type")]
    pub(super) error_type: String,
    pub(super) message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ProviderDelta {
    #[serde(rename = "type")]
    #[serde(default)]
    pub(super) _delta_type: Option<String>,
    #[serde(default)]
    pub(super) text: Option<String>,
    #[serde(default)]
    pub(super) partial_json: Option<String>,
    #[serde(default)]
    pub(super) thinking: Option<String>,
    #[serde(default)]
    pub(super) _signature: Option<String>,
    #[serde(default)]
    pub(super) _choice_index: Option<usize>,
}

#[derive(Debug, Clone)]
pub(super) enum ProviderContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult,
    Thinking {
        thinking: String,
    },
    ThinkingData {
        data: String,
    },
    ServerToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    WebSearchToolResult,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ProviderContentBlockCompat {
    Text {
        text: String,
        #[serde(default)]
        citations: Option<Vec<serde_json::Value>>,
    },
    ToolUse {
        id: String,
        name: String,
        #[serde(default = "default_json_object")]
        input: serde_json::Value,
        #[serde(default)]
        metadata: Option<serde_json::Value>,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    ThinkingData {
        data: String,
    },
    ServerToolUse {
        id: String,
        name: String,
        #[serde(default = "default_json_object")]
        input: serde_json::Value,
    },
    WebSearchToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: serde_json::Value,
    },
}

impl From<ProviderContentBlockCompat> for ProviderContentBlock {
    fn from(value: ProviderContentBlockCompat) -> Self {
        match value {
            ProviderContentBlockCompat::Text { text, citations } => {
                let _ = citations;
                Self::Text { text }
            }
            ProviderContentBlockCompat::ToolUse {
                id,
                name,
                input,
                metadata,
            } => {
                let _ = metadata;
                Self::ToolUse { id, name, input }
            }
            ProviderContentBlockCompat::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let _ = (tool_use_id, content, is_error);
                Self::ToolResult
            }
            ProviderContentBlockCompat::Thinking {
                thinking,
                signature,
            } => {
                let _ = signature;
                Self::Thinking { thinking }
            }
            ProviderContentBlockCompat::ThinkingData { data } => Self::ThinkingData { data },
            ProviderContentBlockCompat::ServerToolUse { id, name, input } => {
                Self::ServerToolUse { id, name, input }
            }
            ProviderContentBlockCompat::WebSearchToolResult {
                tool_use_id,
                content,
            } => {
                let _ = (tool_use_id, content);
                Self::WebSearchToolResult
            }
        }
    }
}

impl<'de> Deserialize<'de> for ProviderContentBlock {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut value = serde_json::Value::deserialize(deserializer)?;
        if let Some(object) = value.as_object_mut()
            && object
                .get("type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|tag| {
                    tag == legacy_external_thinking_tag()
                        || tag == transitional_internal_thinking_tag()
                })
        {
            object.insert(
                "type".to_string(),
                serde_json::Value::String(THINKING_DATA_TAG.to_string()),
            );
        }

        serde_json::from_value::<ProviderContentBlockCompat>(value)
            .map(Into::into)
            .map_err(serde::de::Error::custom)
    }
}

impl StreamParser {
    pub fn finish(&mut self) -> Vec<RuntimeEnvelope> {
        if self.protocol_mode != StreamProtocolMode::ProviderNormalized
            || !self.provider_turn_started
        {
            return Vec::new();
        }

        let mut envelopes = Vec::new();
        let open_blocks = std::mem::take(&mut self.provider_open_blocks);
        for index in open_blocks {
            envelopes.extend(
                self.provider_normalizer_mut()
                    .normalize_ui_update(&UiUpdate::StreamBlockComplete { index }, None),
            );
        }

        let usage = token_usage_from_turn_tokens(self.provider_turn_tokens);
        envelopes.extend(self.provider_normalizer_mut().normalize_ui_update(
            &UiUpdate::TurnComplete,
            Some(TurnEndContext {
                usage,
                changed_files: Vec::new(),
            }),
        ));

        self.provider_turn_started = false;
        self.provider_turn_tokens = TurnTokens::default();
        envelopes
    }

    pub(super) fn normalize_provider_events(
        &mut self,
        events: Vec<ProviderStreamEvent>,
    ) -> Vec<RuntimeEnvelope> {
        if events.is_empty() {
            return Vec::new();
        }

        if self.protocol_mode == StreamProtocolMode::RuntimeEnvelope {
            return vec![self.provider_error_envelope(
                "mixed_sse_protocol",
                "received legacy stream events after RuntimeEnvelope passthrough began".to_string(),
            )];
        }
        self.protocol_mode = StreamProtocolMode::ProviderNormalized;

        let mut envelopes = Vec::new();
        for event in events {
            envelopes.extend(self.normalize_provider_event(event));
        }

        envelopes
    }

    fn normalize_provider_event(&mut self, event: ProviderStreamEvent) -> Vec<RuntimeEnvelope> {
        match event {
            ProviderStreamEvent::MessageStart { message } => {
                let mut envelopes = Vec::new();
                if let Some(metadata) = message_start_metadata(message.metadata) {
                    envelopes.extend(self.emit_server_metadata(metadata));
                }
                if let Some(usage) = message.usage {
                    envelopes.extend(self.emit_usage_update(usage));
                }
                envelopes
            }
            ProviderStreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                let Some((block, initial_delta)) = provider_stream_block(&content_block) else {
                    return Vec::new();
                };

                let mut envelopes: Vec<RuntimeEnvelope> =
                    self.ensure_provider_turn_started().into_iter().collect();
                self.provider_open_blocks.insert(index);
                envelopes.extend(
                    self.provider_normalizer_mut()
                        .normalize_ui_update(&UiUpdate::StreamBlockStart { index, block }, None),
                );
                if let Some(delta) = initial_delta.filter(|delta| !delta.is_empty()) {
                    envelopes.extend(
                        self.provider_normalizer_mut().normalize_ui_update(
                            &UiUpdate::StreamBlockDelta { index, delta },
                            None,
                        ),
                    );
                }
                envelopes
            }
            ProviderStreamEvent::ContentBlockDelta { index, delta } => {
                let Some(block_delta) = provider_block_delta(&delta) else {
                    return Vec::new();
                };

                let mut envelopes: Vec<RuntimeEnvelope> =
                    self.ensure_provider_turn_started().into_iter().collect();
                if matches!(block_delta, ProviderBlockDelta::Text(_))
                    && !self.provider_open_blocks.contains(&index)
                {
                    self.provider_open_blocks.insert(index);
                    envelopes.extend(self.provider_normalizer_mut().normalize_ui_update(
                        &UiUpdate::StreamBlockStart {
                            index,
                            block: StreamBlock::Thinking {
                                content: String::new(),
                                collapsed: false,
                            },
                        },
                        None,
                    ));
                }

                let delta = match block_delta {
                    ProviderBlockDelta::Text(delta) | ProviderBlockDelta::ToolArguments(delta) => {
                        delta
                    }
                };
                envelopes.extend(
                    self.provider_normalizer_mut()
                        .normalize_ui_update(&UiUpdate::StreamBlockDelta { index, delta }, None),
                );
                envelopes
            }
            ProviderStreamEvent::ContentBlockStop { index } => {
                if !self.provider_open_blocks.remove(&index) {
                    return Vec::new();
                }
                self.provider_normalizer_mut()
                    .normalize_ui_update(&UiUpdate::StreamBlockComplete { index }, None)
            }
            ProviderStreamEvent::MessageDelta { delta, usage } => {
                let mut envelopes = Vec::new();
                if let Some(metadata) = delta.metadata {
                    envelopes.extend(self.emit_server_metadata(metadata));
                }
                if let Some(usage) = usage {
                    envelopes.extend(self.emit_usage_update(usage));
                }
                envelopes
            }
            ProviderStreamEvent::MessageStop
            | ProviderStreamEvent::Ping
            | ProviderStreamEvent::Unknown => Vec::new(),
            ProviderStreamEvent::Error { error } => {
                vec![
                    self.provider_normalizer_mut()
                        .emit_event(RuntimeEvent::Error {
                            code: error.error_type,
                            message: error.message,
                            recoverable: true,
                        }),
                ]
            }
        }
    }

    fn ensure_provider_turn_started(&mut self) -> Option<RuntimeEnvelope> {
        if self.provider_turn_started {
            return None;
        }

        self.provider_turn_started = true;
        Some(self.provider_normalizer_mut().start_turn(1, None))
    }

    fn provider_normalizer_mut(&mut self) -> &mut RuntimeEnvelopeNormalizer {
        self.provider_normalizer
            .get_or_insert_with(|| RuntimeEnvelopeNormalizer::new(next_stream_task_id()))
    }

    fn emit_server_metadata(&mut self, metadata: StreamChunkMetadata) -> Vec<RuntimeEnvelope> {
        let mut envelopes = self
            .ensure_provider_turn_started()
            .into_iter()
            .collect::<Vec<_>>();
        envelopes.extend(
            self.provider_normalizer_mut()
                .normalize_ui_update(&UiUpdate::ServerMetadata(Box::new(metadata)), None),
        );
        envelopes
    }

    fn emit_usage_update(&mut self, usage: ApiUsage) -> Vec<RuntimeEnvelope> {
        accumulate_turn_tokens(&mut self.provider_turn_tokens, &usage);

        let mut envelopes = self
            .ensure_provider_turn_started()
            .into_iter()
            .collect::<Vec<_>>();
        if let Some(usage) = token_usage_from_api_usage(&usage) {
            envelopes.push(
                self.provider_normalizer_mut()
                    .emit_event(RuntimeEvent::UsageUpdated { usage }),
            );
        }
        envelopes
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
        let _ = self.ensure_provider_turn_started();
        self.provider_normalizer_mut()
            .emit_event(RuntimeEvent::Error {
                code: code.to_string(),
                message,
                recoverable: true,
            })
    }
}

#[derive(Debug, Clone)]
enum ProviderBlockDelta {
    Text(String),
    ToolArguments(String),
}

fn default_json_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

fn next_stream_task_id() -> String {
    let id = NEXT_STREAM_TASK_ID.fetch_add(1, Ordering::Relaxed);
    format!("api_stream_{id}")
}

fn provider_stream_block(
    content_block: &ProviderContentBlock,
) -> Option<(StreamBlock, Option<String>)> {
    match content_block {
        ProviderContentBlock::Text { text } => Some((
            StreamBlock::Thinking {
                content: String::new(),
                collapsed: false,
            },
            Some(text.clone()),
        )),
        ProviderContentBlock::Thinking { thinking } => Some((
            StreamBlock::Thinking {
                content: String::new(),
                collapsed: false,
            },
            Some(thinking.clone()),
        )),
        ProviderContentBlock::ThinkingData { data } => Some((
            StreamBlock::Thinking {
                content: String::new(),
                collapsed: false,
            },
            Some(data.clone()),
        )),
        ProviderContentBlock::ToolUse { id, name, input }
        | ProviderContentBlock::ServerToolUse { id, name, input } => Some((
            StreamBlock::ToolCall {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
                status: ToolStatus::Pending,
            },
            None,
        )),
        ProviderContentBlock::ToolResult | ProviderContentBlock::WebSearchToolResult => None,
    }
}

fn provider_block_delta(delta: &ProviderDelta) -> Option<ProviderBlockDelta> {
    if let Some(text) = delta.text.as_ref().filter(|text| !text.is_empty()) {
        return Some(ProviderBlockDelta::Text(text.clone()));
    }
    if let Some(thinking) = delta
        .thinking
        .as_ref()
        .filter(|thinking| !thinking.is_empty())
    {
        return Some(ProviderBlockDelta::Text(thinking.clone()));
    }
    delta
        .partial_json
        .as_ref()
        .filter(|partial_json| !partial_json.is_empty())
        .map(|partial_json| ProviderBlockDelta::ToolArguments(partial_json.clone()))
}

fn message_start_metadata(metadata: Option<StreamChunkMetadata>) -> Option<StreamChunkMetadata> {
    let mut metadata = metadata?;
    metadata.prompt_progress = None;
    metadata.timings = None;

    (metadata.object.is_some()
        || metadata.created.is_some()
        || metadata.system_fingerprint.is_some()
        || metadata.service_tier.is_some()
        || metadata.choice_index.is_some()
        || metadata.logprobs.is_some())
    .then_some(metadata)
}

fn token_usage_from_api_usage(usage: &ApiUsage) -> Option<TokenUsageEnvelope> {
    let usage = TokenUsageEnvelope {
        input: usage.input_tokens.unwrap_or(0),
        output: usage.output_tokens.unwrap_or(0),
        estimated: false,
        cache_creation_input: usage.cache_creation_input_tokens.unwrap_or(0),
        cache_read_input: usage.cache_read_input_tokens.unwrap_or(0),
    };

    (!usage.input.eq(&0)
        || !usage.output.eq(&0)
        || !usage.cache_creation_input.eq(&0)
        || !usage.cache_read_input.eq(&0))
    .then_some(usage)
}

fn token_usage_from_turn_tokens(tokens: TurnTokens) -> Option<TokenUsageEnvelope> {
    if tokens.is_zero() {
        None
    } else {
        Some(TokenUsageEnvelope {
            input: tokens.input,
            output: tokens.output,
            estimated: tokens.estimated,
            cache_creation_input: tokens.cache_creation_input_tokens,
            cache_read_input: tokens.cache_read_input_tokens,
        })
    }
}

fn accumulate_turn_tokens(turn_tokens: &mut TurnTokens, usage: &ApiUsage) {
    if let Some(input) = usage.input_tokens {
        turn_tokens.input = turn_tokens.input.saturating_add(input);
    }
    if let Some(output) = usage.output_tokens {
        turn_tokens.output = turn_tokens.output.saturating_add(output);
    }
    if let Some(cache_creation) = usage.cache_creation_input_tokens {
        turn_tokens.cache_creation_input_tokens = turn_tokens
            .cache_creation_input_tokens
            .saturating_add(cache_creation);
    }
    if let Some(cache_read) = usage.cache_read_input_tokens {
        turn_tokens.cache_read_input_tokens = turn_tokens
            .cache_read_input_tokens
            .saturating_add(cache_read);
    }
}
