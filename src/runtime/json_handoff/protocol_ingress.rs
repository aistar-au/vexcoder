use super::derived::{empty_json_object, token_usage_from_turn_tokens};
use super::{RuntimeEnvelope, RuntimeEnvelopeNormalizer, RuntimeEvent, TurnEndContext};
use crate::api::stream::MAX_TOOL_CALL_INDEX;
use crate::api::stream::chat_compat::{
    ChatCompatChoice, ChatCompatChunk, ChatCompatDelta, ChatCompatPayload, ChatCompatToolCallDelta,
};
use crate::api::stream::provider::{ProviderDelta, ProviderStreamEvent};
use crate::state::{StreamBlock, ToolStatus};
use crate::types::{ApiUsage, StreamChunkMetadata};
use crate::usage::TurnTokens;
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct ProtocolIngressState {
    turn_started: bool,
    // Provider block indices come from ordered JSON arrays. RFC 8259 treats
    // arrays as ordered sequences and objects as unordered collections, so
    // turn-final completion must follow ascending index order even if deltas
    // arrive out of order.
    open_blocks: BTreeSet<usize>,
    turn_tokens: TurnTokens,
    chat_compat_message_started: bool,
    chat_compat_tools: Vec<PendingChatCompatToolState>,
}

#[derive(Debug, Clone, Default)]
struct PendingChatCompatToolState {
    id: String,
    name: String,
    pending_arguments: String,
    started: bool,
    stopped: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ProviderContentBlockCompat {
    Text {
        text: String,
        #[serde(default)]
        citations: Vec<serde_json::Value>,
    },
    ToolUse {
        id: String,
        name: String,
        #[serde(default = "empty_json_object")]
        input: serde_json::Value,
        #[serde(default)]
        metadata: Option<serde_json::Value>,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: serde_json::Value,
        #[serde(default)]
        is_error: bool,
    },
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
    },
    ThinkingData {
        data: String,
    },
    ServerToolUse {
        id: String,
        name: String,
        #[serde(default = "empty_json_object")]
        input: serde_json::Value,
    },
    WebSearchToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: serde_json::Value,
    },
}

#[derive(Debug, Clone)]
enum ProviderContentBlock {
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

#[derive(Debug, Clone)]
enum ProviderBlockDelta {
    Text(String),
    ToolArguments(String),
}

impl RuntimeEnvelopeNormalizer {
    pub(crate) fn finish_protocol_ingress_turn(&mut self) -> Vec<RuntimeEnvelope> {
        if !self.protocol_ingress.turn_started {
            return Vec::new();
        }

        let mut envelopes = Vec::new();
        let open_blocks = std::mem::take(&mut self.protocol_ingress.open_blocks);
        for index in open_blocks {
            envelopes.extend(
                self.normalize_ui_update(&super::UiUpdate::StreamBlockComplete { index }, None),
            );
        }

        let usage =
            token_usage_from_turn_tokens(std::mem::take(&mut self.protocol_ingress.turn_tokens));
        envelopes.extend(self.normalize_ui_update(
            &super::UiUpdate::TurnComplete,
            Some(TurnEndContext {
                usage,
                changed_files: Vec::new(),
            }),
        ));

        self.protocol_ingress = ProtocolIngressState::default();
        envelopes
    }

    pub(crate) fn normalize_provider_stream_event(
        &mut self,
        event: ProviderStreamEvent,
    ) -> Vec<RuntimeEnvelope> {
        match event {
            ProviderStreamEvent::MessageStart { message } => {
                let mut envelopes = Vec::new();
                if let Some(metadata) = message_start_metadata(message.metadata) {
                    envelopes.extend(self.emit_provider_metadata(metadata));
                }
                if let Some(usage) = message.usage {
                    envelopes.extend(self.emit_provider_usage(usage));
                }
                envelopes
            }
            ProviderStreamEvent::ContentBlockStart {
                index,
                content_block,
            } => match decode_provider_content_block(content_block) {
                Ok(content_block) => self.open_provider_stream_block(index, content_block),
                Err(err) => vec![self.emit_event(RuntimeEvent::Error {
                    code: "provider_content_block_start_decode".to_string(),
                    message: format!(
                        "failed to decode provider content_block_start event at index {index}: {err}"
                    ),
                    recoverable: true,
                })],
            },
            ProviderStreamEvent::ContentBlockDelta { index, delta } => provider_block_delta(&delta)
                .map(|delta| self.apply_provider_stream_delta(index, delta))
                .unwrap_or_default(),
            ProviderStreamEvent::ContentBlockStop { index } => self.close_provider_stream_block(index),
            ProviderStreamEvent::MessageDelta { delta, usage } => {
                let mut envelopes = Vec::new();
                if let Some(metadata) = delta.metadata {
                    envelopes.extend(self.emit_provider_metadata(metadata));
                }
                if let Some(usage) = usage {
                    envelopes.extend(self.emit_provider_usage(usage));
                }
                envelopes
            }
            ProviderStreamEvent::MessageStop
            | ProviderStreamEvent::Ping
            | ProviderStreamEvent::Unknown => Vec::new(),
            ProviderStreamEvent::Error { error } => vec![self.emit_event(RuntimeEvent::Error {
                code: error.error_type,
                message: error.message,
                recoverable: true,
            })],
        }
    }

    pub(crate) fn normalize_chat_compat_payload(
        &mut self,
        payload: ChatCompatPayload,
    ) -> Vec<RuntimeEnvelope> {
        match payload {
            ChatCompatPayload::Done => {
                let envelopes = self.close_chat_compat_tool_blocks();
                self.protocol_ingress.chat_compat_message_started = false;
                self.protocol_ingress.chat_compat_tools.clear();
                envelopes
            }
            ChatCompatPayload::Chunk(chunk) => self.normalize_chat_compat_chunk(*chunk),
        }
    }

    pub(crate) fn emit_protocol_ingress_error(
        &mut self,
        code: String,
        message: String,
    ) -> RuntimeEnvelope {
        let _ = self.ensure_protocol_ingress_turn_started();
        self.emit_event(RuntimeEvent::Error {
            code,
            message,
            recoverable: true,
        })
    }

    fn ensure_protocol_ingress_turn_started(&mut self) -> Vec<RuntimeEnvelope> {
        if self.protocol_ingress.turn_started {
            return Vec::new();
        }

        let envelope = self.start_turn(1, None);
        self.protocol_ingress.turn_started = true;
        vec![envelope]
    }

    fn open_provider_stream_block(
        &mut self,
        index: usize,
        content_block: ProviderContentBlock,
    ) -> Vec<RuntimeEnvelope> {
        let Some((block, initial_delta)) = provider_stream_block(&content_block) else {
            return Vec::new();
        };

        let mut envelopes = self.ensure_protocol_ingress_turn_started();
        self.protocol_ingress.open_blocks.insert(index);
        envelopes.extend(
            self.normalize_ui_update(&super::UiUpdate::StreamBlockStart { index, block }, None),
        );
        if let Some(delta) = initial_delta.filter(|delta| !delta.is_empty()) {
            envelopes.extend(
                self.normalize_ui_update(&super::UiUpdate::StreamBlockDelta { index, delta }, None),
            );
        }
        envelopes
    }

    fn apply_provider_stream_delta(
        &mut self,
        index: usize,
        block_delta: ProviderBlockDelta,
    ) -> Vec<RuntimeEnvelope> {
        let mut envelopes = self.ensure_protocol_ingress_turn_started();
        if matches!(block_delta, ProviderBlockDelta::Text(_))
            && !self.protocol_ingress.open_blocks.contains(&index)
        {
            self.protocol_ingress.open_blocks.insert(index);
            envelopes.extend(self.normalize_ui_update(
                &super::UiUpdate::StreamBlockStart {
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
            ProviderBlockDelta::Text(delta) | ProviderBlockDelta::ToolArguments(delta) => delta,
        };
        envelopes.extend(
            self.normalize_ui_update(&super::UiUpdate::StreamBlockDelta { index, delta }, None),
        );
        envelopes
    }

    fn close_provider_stream_block(&mut self, index: usize) -> Vec<RuntimeEnvelope> {
        if !self.protocol_ingress.open_blocks.remove(&index) {
            return Vec::new();
        }

        self.normalize_ui_update(&super::UiUpdate::StreamBlockComplete { index }, None)
    }

    fn emit_provider_metadata(&mut self, metadata: StreamChunkMetadata) -> Vec<RuntimeEnvelope> {
        let mut envelopes = self.ensure_protocol_ingress_turn_started();
        envelopes.extend(
            self.normalize_ui_update(&super::UiUpdate::ServerMetadata(Box::new(metadata)), None),
        );
        envelopes
    }

    fn emit_provider_usage(&mut self, usage: ApiUsage) -> Vec<RuntimeEnvelope> {
        accumulate_turn_tokens(&mut self.protocol_ingress.turn_tokens, &usage);

        let mut envelopes = self.ensure_protocol_ingress_turn_started();
        if let Some(usage) = token_usage_from_api_usage(&usage) {
            envelopes.push(self.emit_event(RuntimeEvent::UsageUpdated { usage }));
        }
        envelopes
    }

    fn normalize_chat_compat_chunk(&mut self, mut chunk: ChatCompatChunk) -> Vec<RuntimeEnvelope> {
        let mut envelopes = self.emit_chat_compat_message_start(&chunk);

        if let Some(mut usage) = chunk.usage.take() {
            if usage.service_tier.is_none() {
                usage.service_tier = chunk.service_tier.clone();
            }
            if let Some(metadata) = chat_compat_metadata(&chunk, None, None) {
                envelopes.extend(self.emit_provider_metadata(metadata));
            }
            envelopes.extend(self.emit_provider_usage(usage));
        }

        if chunk.choices.is_empty() {
            return envelopes;
        }

        for choice in std::mem::take(&mut chunk.choices) {
            let ChatCompatChoice {
                index: choice_index,
                delta,
                finish_reason,
                logprobs,
            } = choice;
            let ChatCompatDelta {
                role: _,
                content,
                refusal,
                tool_calls,
            } = delta;

            if let Some(content) = content {
                envelopes
                    .extend(self.apply_provider_stream_delta(0, ProviderBlockDelta::Text(content)));
            }

            if let Some(tool_calls) = tool_calls {
                for tool_call in tool_calls {
                    envelopes.extend(self.apply_chat_compat_tool_delta(choice_index, tool_call));
                }
            }

            let has_logprobs = logprobs.is_some();
            let metadata = chat_compat_metadata(&chunk, choice_index, logprobs);
            let has_server_progress = metadata
                .as_ref()
                .is_some_and(|md| md.prompt_progress.is_some() || md.timings.is_some());

            if (refusal.is_some() || finish_reason.is_some() || has_logprobs || has_server_progress)
                && let Some(metadata) = metadata
            {
                envelopes.extend(self.emit_provider_metadata(metadata));
            }

            if finish_reason.is_some() {
                envelopes.extend(self.close_chat_compat_tool_blocks());
            }
        }

        envelopes
    }

    fn emit_chat_compat_message_start(&mut self, chunk: &ChatCompatChunk) -> Vec<RuntimeEnvelope> {
        if self.protocol_ingress.chat_compat_message_started {
            return Vec::new();
        }

        let role = chunk
            .choices
            .iter()
            .find_map(|choice| choice.delta.role.clone());
        let metadata = chat_compat_metadata(chunk, None, None);
        let has_message_fields =
            chunk.id.is_some() || chunk.model.is_some() || role.is_some() || metadata.is_some();
        if !has_message_fields {
            return Vec::new();
        }

        self.protocol_ingress.chat_compat_message_started = true;
        message_start_metadata(metadata)
            .map(|metadata| self.emit_provider_metadata(metadata))
            .unwrap_or_default()
    }

    fn apply_chat_compat_tool_delta(
        &mut self,
        choice_index: Option<usize>,
        tool_call: ChatCompatToolCallDelta,
    ) -> Vec<RuntimeEnvelope> {
        let raw_index = tool_call.index.unwrap_or(0).min(MAX_TOOL_CALL_INDEX);
        let block_index = raw_index.saturating_add(1);
        self.ensure_chat_compat_tool_slot(block_index);

        let mut start_block = None;
        let mut partial_json = None;
        {
            let state = &mut self.protocol_ingress.chat_compat_tools[block_index];
            let _ = (choice_index, tool_call.call_type);

            if let Some(id) = tool_call.id
                && !id.is_empty()
            {
                state.id = id;
            }
            if let Some(function) = tool_call.function {
                if let Some(name) = function.name
                    && !name.is_empty()
                {
                    state.name = name;
                }
                if let Some(arguments) = function.arguments {
                    state.pending_arguments.push_str(&arguments);
                }
            }

            if !state.started && !state.name.is_empty() {
                let id = if state.id.is_empty() {
                    format!("toolu_chat_compat_{block_index}")
                } else {
                    state.id.clone()
                };
                start_block = Some(ProviderContentBlock::ToolUse {
                    id,
                    name: state.name.clone(),
                    input: empty_json_object(),
                });
                state.started = true;
            }

            if state.started && !state.pending_arguments.is_empty() {
                partial_json = Some(std::mem::take(&mut state.pending_arguments));
            }
        }

        let mut envelopes = Vec::new();
        if let Some(content_block) = start_block {
            envelopes.extend(self.open_provider_stream_block(block_index, content_block));
        }
        if let Some(partial_json) = partial_json {
            envelopes.extend(self.apply_provider_stream_delta(
                block_index,
                ProviderBlockDelta::ToolArguments(partial_json),
            ));
        }
        envelopes
    }

    fn ensure_chat_compat_tool_slot(&mut self, index: usize) {
        let required = index.saturating_add(1);
        if self.protocol_ingress.chat_compat_tools.len() < required {
            self.protocol_ingress
                .chat_compat_tools
                .resize_with(required, PendingChatCompatToolState::default);
        }
    }

    fn close_chat_compat_tool_blocks(&mut self) -> Vec<RuntimeEnvelope> {
        let mut to_close = Vec::new();
        for (index, state) in self
            .protocol_ingress
            .chat_compat_tools
            .iter_mut()
            .enumerate()
        {
            if index == 0 {
                continue;
            }
            if state.started && !state.stopped {
                state.stopped = true;
                to_close.push(index);
            }
        }

        let mut envelopes = Vec::new();
        for index in to_close {
            envelopes.extend(self.close_provider_stream_block(index));
        }
        envelopes
    }
}

fn decode_provider_content_block(value: serde_json::Value) -> Result<ProviderContentBlock, String> {
    let block_type = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>")
        .to_string();

    serde_json::from_value::<ProviderContentBlockCompat>(value)
        .map(Into::into)
        .map_err(|err| format!("type={block_type}: {err}"))
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
    if let Some(text) = delta.text.as_deref()
        && !text.is_empty()
    {
        return Some(ProviderBlockDelta::Text(text.to_string()));
    }
    if let Some(thinking) = delta.thinking.as_deref()
        && !thinking.is_empty()
    {
        return Some(ProviderBlockDelta::Text(thinking.to_string()));
    }
    if let Some(partial_json) = delta.partial_json.as_deref()
        && !partial_json.is_empty()
    {
        return Some(ProviderBlockDelta::ToolArguments(partial_json.to_string()));
    }

    None
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

fn chat_compat_metadata(
    chunk: &ChatCompatChunk,
    choice_index: Option<usize>,
    logprobs: Option<serde_json::Value>,
) -> Option<StreamChunkMetadata> {
    let metadata = StreamChunkMetadata {
        object: chunk.object.clone(),
        created: chunk.created,
        system_fingerprint: chunk.system_fingerprint.clone(),
        service_tier: chunk.service_tier.clone(),
        choice_index,
        logprobs,
        prompt_progress: chunk.prompt_progress.clone(),
        timings: chunk.timings.clone(),
    };
    (metadata.object.is_some()
        || metadata.created.is_some()
        || metadata.system_fingerprint.is_some()
        || metadata.service_tier.is_some()
        || metadata.choice_index.is_some()
        || metadata.logprobs.is_some()
        || metadata.prompt_progress.is_some()
        || metadata.timings.is_some())
    .then_some(metadata)
}

fn token_usage_from_api_usage(usage: &ApiUsage) -> Option<super::TokenUsageEnvelope> {
    let usage = super::TokenUsageEnvelope {
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
