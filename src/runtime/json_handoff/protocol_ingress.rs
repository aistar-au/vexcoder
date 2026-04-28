use super::derived::{empty_json_object, token_usage_from_turn_tokens};
use super::{PulseEndContext, RuntimeEnvelope, RuntimeEnvelopeNormalizer, RuntimeSignal};
use crate::api::stream::chat_compat::{
    ChatCompatChoice, ChatCompatChunk, ChatCompatDelta, ChatCompatFunctionArguments,
    ChatCompatPayload, ChatCompatToolCallDelta,
};
use crate::api::stream::provider::{ProviderDelta, ProviderStreamItem};
use crate::api::stream::{MAX_TOOL_CALL_INDEX, NormalisedChunk, StreamTextNormaliser};
use crate::state::{StreamBlock, ToolStatus};
use crate::types::{ApiUsage, StreamChunkMetadata};
use crate::usage::PulseTokens;
use opentelemetry::KeyValue;
use opentelemetry_semantic_conventions::attribute as semconv;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct ProtocolIngressState {
    turn_started: bool,

    open_blocks: BTreeSet<usize>,
    turn_tokens: PulseTokens,
    chat_compat_message_started: bool,
    tagged_text_normalisers: BTreeMap<usize, StreamTextNormaliser>,
    next_tagged_tool_block_index: usize,

    chat_compat_tool_lifecycles: BTreeMap<usize, PendingChatCompatToolState>,
    // Set to true the first time a native chat-compat tool call block opens.
    // When true, XML tool calls synthesised from text content are suppressed so that
    // models which echo their tool call as both a native tool_calls entry AND as XML
    // in the text content do not produce duplicate tool call blocks.
    has_native_tool_calls: bool,

    chat_compat_stream_terminated: bool,
}

// Start tagged-XML tool blocks at 1, immediately after the chat-compat text block (always index 0).
// Index space: 0 = text, 1..=N = tagged XML tool calls.
// Native chat-compat tool calls use raw_index+1 (1..=MAX_TOOL_CALL_INDEX+1), but when the server
// sends native tool_calls the has_native_tool_calls flag suppresses XML tool call synthesis,
// so collisions cannot occur in practice (see emit_tagged_tool_call_from_text).
// Local coding models (tool-parser implementations and local inference server PRs) confirm
// that models either emit XML in content OR use native tool_calls — never both.
const TAGGED_TOOL_BLOCK_START_INDEX: usize = 1;

impl Default for ProtocolIngressState {
    fn default() -> Self {
        Self {
            turn_started: false,
            open_blocks: BTreeSet::new(),
            turn_tokens: PulseTokens::default(),
            chat_compat_message_started: false,
            tagged_text_normalisers: BTreeMap::new(),
            next_tagged_tool_block_index: TAGGED_TOOL_BLOCK_START_INDEX,
            chat_compat_tool_lifecycles: BTreeMap::new(),
            has_native_tool_calls: false,
            chat_compat_stream_terminated: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PendingChatCompatToolState {
    id: String,
    name: String,
    pending_arguments: String,
    materialized_arguments: Option<serde_json::Value>,
    lifecycle: ToolCallLifecycle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ToolCallLifecycle {
    #[default]
    Discovered,
    Opened,
    Closed,
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

fn provider_block_kind(block: &ProviderContentBlock) -> &'static str {
    match block {
        ProviderContentBlock::Text { .. } => "text",
        ProviderContentBlock::ToolUse { .. } => "tool_use",
        ProviderContentBlock::ToolResult => "tool_result",
        ProviderContentBlock::Thinking { .. } => "thinking",
        ProviderContentBlock::ThinkingData { .. } => "thinking_data",
        ProviderContentBlock::ServerToolUse { .. } => "server_tool_use",
        ProviderContentBlock::WebSearchToolResult => "web_search_tool_result",
    }
}

fn provider_delta_kind(delta: &ProviderBlockDelta) -> &'static str {
    match delta {
        ProviderBlockDelta::Text(_) => "text",
        ProviderBlockDelta::ToolArguments(_) => "tool_arguments",
    }
}

fn annotate_current_tool_span(tool_name: &str, tool_call_id: &str) {
    crate::observability::set_span_attributes(
        &tracing::Span::current(),
        [
            KeyValue::new(semconv::GEN_AI_TOOL_NAME, tool_name.to_string()),
            KeyValue::new(semconv::GEN_AI_TOOL_CALL_ID, tool_call_id.to_string()),
        ],
    );
}

impl RuntimeEnvelopeNormalizer {
    pub(crate) fn finish_protocol_ingress_turn(&mut self) -> Vec<RuntimeEnvelope> {
        if !self.protocol_ingress.turn_started {
            return Vec::new();
        }

        let mut envelopes = Vec::new();

        // Flush first-layer normaliser buffers before closing blocks. Text held in the
        // pending buffer (e.g. a trailing `<` held as a partial-tag candidate) is emitted
        // here. Do this before taking open_blocks so that any new tool blocks synthesised
        // by emit_tagged_tool_call_from_text are opened and closed atomically.
        let normalisers = std::mem::take(&mut self.protocol_ingress.tagged_text_normalisers);
        for (index, mut normaliser) in normalisers {
            let chunks = normaliser.finish();
            if !chunks.is_empty() {
                envelopes.extend(self.emit_normalised_provider_chunks(index, chunks));
            }
        }

        let open_blocks = std::mem::take(&mut self.protocol_ingress.open_blocks);
        for index in open_blocks {
            envelopes.extend(
                self.normalize_ui_update(&super::UiUpdate::StreamBlockComplete { index }, None),
            );
        }

        let usage =
            token_usage_from_turn_tokens(std::mem::take(&mut self.protocol_ingress.turn_tokens));
        if let Some(usage) = &usage {
            crate::observability::set_span_attributes(
                &tracing::Span::current(),
                [
                    KeyValue::new(
                        semconv::GEN_AI_USAGE_INPUT_TOKENS,
                        i64::try_from(usage.input).unwrap_or(i64::MAX),
                    ),
                    KeyValue::new(
                        semconv::GEN_AI_USAGE_OUTPUT_TOKENS,
                        i64::try_from(usage.output).unwrap_or(i64::MAX),
                    ),
                ],
            );
        }
        envelopes.extend(self.normalize_ui_update(
            &super::UiUpdate::PulseComplete,
            Some(PulseEndContext {
                usage,
                changed_files: Vec::new(),
            }),
        ));

        self.protocol_ingress = ProtocolIngressState::default();
        envelopes
    }

    pub(crate) fn normalize_provider_stream_item(
        &mut self,
        item: ProviderStreamItem,
    ) -> Vec<RuntimeEnvelope> {
        match item {
            ProviderStreamItem::MessageStart { message } => {
                let mut envelopes = Vec::new();
                if let Some(metadata) = message_start_metadata(message.metadata) {
                    envelopes.extend(self.emit_provider_metadata(metadata));
                }
                if let Some(usage) = message.usage {
                    envelopes.extend(self.emit_provider_usage(usage));
                }
                envelopes
            }
            ProviderStreamItem::ContentBlockStart {
                index,
                content_block,
            } => match decode_provider_content_block(content_block.clone()) {
                Ok(content_block) => self.open_provider_stream_block(index, content_block),
                Err(err) => {
                    let raw = serde_json::to_string(&content_block)
                        .unwrap_or_else(|_| "<non-serializable>".to_string());
                    let truncated = if raw.len() > 512 {
                        format!("{}…({}B total)", &raw[..512], raw.len())
                    } else {
                        raw
                    };
                    tracing::warn!(
                        target: "vex::protocol",
                        index,
                        error = %err,
                        raw_payload = %truncated,
                        "failed to decode provider content_block_start; raw payload logged for diagnosis"
                    );
                    vec![self.emit_signal(RuntimeSignal::Error {
                        code: "provider_content_block_start_decode".to_string(),
                        message: format!(
                            "failed to decode provider content_block_start item at index {index}: {err}"
                        ),
                        recoverable: true,
                    })]
                }
            },
            ProviderStreamItem::ContentBlockDelta { index, delta } => provider_block_delta(&delta)
                .map(|delta| self.apply_provider_stream_delta(index, delta))
                .unwrap_or_default(),
            ProviderStreamItem::ContentBlockStop { index } => {
                self.close_provider_stream_block(index)
            }
            ProviderStreamItem::MessageDelta { delta, usage } => {
                let mut envelopes = Vec::new();
                if let Some(metadata) = delta.metadata {
                    envelopes.extend(self.emit_provider_metadata(metadata));
                }
                if let Some(usage) = usage {
                    envelopes.extend(self.emit_provider_usage(usage));
                }
                envelopes
            }
            ProviderStreamItem::MessageStop
            | ProviderStreamItem::Ping
            | ProviderStreamItem::Unknown => Vec::new(),
            ProviderStreamItem::Error { error } => vec![self.emit_signal(RuntimeSignal::Error {
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
                tracing::trace!(
                    target: "vex::protocol",
                    "chat-compatible stream terminated with [DONE]"
                );
                let envelopes = self.close_chat_compat_tool_blocks();
                self.protocol_ingress.chat_compat_message_started = false;
                self.protocol_ingress.chat_compat_stream_terminated = true;
                self.protocol_ingress.chat_compat_tool_lifecycles.clear();
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
        self.emit_signal(RuntimeSignal::Error {
            code,
            message,
            recoverable: true,
        })
    }

    pub(crate) fn protocol_stream_terminated(&self) -> bool {
        self.protocol_ingress.chat_compat_stream_terminated
    }

    fn ensure_protocol_ingress_turn_started(&mut self) -> Vec<RuntimeEnvelope> {
        if self.protocol_ingress.turn_started {
            return Vec::new();
        }

        tracing::trace!(target: "vex::protocol", "protocol ingress pulse started");
        let envelope = self.start_pulse(1, None);
        self.protocol_ingress.turn_started = true;
        vec![envelope]
    }

    fn open_provider_stream_block(
        &mut self,
        index: usize,
        content_block: ProviderContentBlock,
    ) -> Vec<RuntimeEnvelope> {
        let needs_text_normaliser = matches!(
            content_block,
            ProviderContentBlock::Text { .. }
                | ProviderContentBlock::Thinking { .. }
                | ProviderContentBlock::ThinkingData { .. }
        );
        if let ProviderContentBlock::ToolUse { id, name, .. }
        | ProviderContentBlock::ServerToolUse { id, name, .. } = &content_block
        {
            annotate_current_tool_span(name, id);
        }
        tracing::trace!(
            target: "vex::protocol",
            index,
            block_kind = provider_block_kind(&content_block),
            "opening provider stream block"
        );
        let Some((block, initial_delta)) = provider_stream_block(&content_block) else {
            return Vec::new();
        };

        let mut envelopes = self.ensure_protocol_ingress_turn_started();
        self.protocol_ingress.open_blocks.insert(index);
        if needs_text_normaliser {
            self.protocol_ingress
                .tagged_text_normalisers
                .entry(index)
                .or_default();
        }
        envelopes.extend(
            self.normalize_ui_update(&super::UiUpdate::StreamBlockStart { index, block }, None),
        );
        if let Some(delta) = initial_delta.filter(|delta| !delta.is_empty()) {
            if needs_text_normaliser {
                envelopes.extend(self.apply_normalised_provider_text(index, &delta));
            } else {
                envelopes.extend(self.normalize_ui_update(
                    &super::UiUpdate::StreamBlockDelta { index, delta },
                    None,
                ));
            }
        }
        envelopes
    }

    fn apply_provider_stream_delta(
        &mut self,
        index: usize,
        block_delta: ProviderBlockDelta,
    ) -> Vec<RuntimeEnvelope> {
        let is_text_delta = matches!(&block_delta, ProviderBlockDelta::Text(_));
        let delta_kind = provider_delta_kind(&block_delta);
        let delta = match block_delta {
            ProviderBlockDelta::Text(delta) | ProviderBlockDelta::ToolArguments(delta) => delta,
        };
        if delta.is_empty() {
            tracing::trace!(
                target: "vex::protocol",
                index,
                delta_kind,
                "ignoring empty provider stream delta"
            );
            return Vec::new();
        }

        let mut envelopes = self.ensure_protocol_ingress_turn_started();
        if is_text_delta && !self.protocol_ingress.open_blocks.contains(&index) {
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
        tracing::trace!(
            target: "vex::protocol",
            index,
            delta_kind,
            delta_bytes = delta.len(),
            "applying provider stream delta"
        );
        if is_text_delta {
            envelopes.extend(self.apply_normalised_provider_text(index, &delta));
        } else {
            envelopes.extend(
                self.normalize_ui_update(&super::UiUpdate::StreamBlockDelta { index, delta }, None),
            );
        }
        envelopes
    }

    fn close_provider_stream_block(&mut self, index: usize) -> Vec<RuntimeEnvelope> {
        if !self.protocol_ingress.open_blocks.remove(&index) {
            return Vec::new();
        }

        let mut envelopes = Vec::new();
        if let Some(mut normaliser) = self.protocol_ingress.tagged_text_normalisers.remove(&index) {
            envelopes.extend(self.emit_normalised_provider_chunks(index, normaliser.finish()));
        }

        tracing::trace!(
            target: "vex::protocol",
            index,
            "closing provider stream block"
        );
        envelopes.extend(
            self.normalize_ui_update(&super::UiUpdate::StreamBlockComplete { index }, None),
        );
        envelopes
    }

    fn apply_normalised_provider_text(
        &mut self,
        index: usize,
        delta: &str,
    ) -> Vec<RuntimeEnvelope> {
        let chunks = self
            .protocol_ingress
            .tagged_text_normalisers
            .entry(index)
            .or_default()
            .normalise(delta);
        self.emit_normalised_provider_chunks(index, chunks)
    }

    fn emit_normalised_provider_chunks(
        &mut self,
        index: usize,
        chunks: Vec<NormalisedChunk>,
    ) -> Vec<RuntimeEnvelope> {
        let mut envelopes = Vec::new();
        for chunk in chunks {
            match chunk {
                NormalisedChunk::Text(text) => {
                    envelopes.extend(self.normalize_ui_update(
                        &super::UiUpdate::StreamBlockDelta { index, delta: text },
                        None,
                    ));
                }
                NormalisedChunk::TaggedToolCall { name, input } => {
                    envelopes.extend(self.emit_tagged_tool_call_from_text(name, input));
                }
            }
        }
        envelopes
    }

    fn emit_tagged_tool_call_from_text(
        &mut self,
        name: String,
        input: serde_json::Value,
    ) -> Vec<RuntimeEnvelope> {
        // Suppress XML-derived tool calls when the server has already sent native tool_calls
        // in the same turn.  Some local models echo the tool call both as a native entry
        // in tool_calls AND as XML inside the content field; suppressing here prevents
        // duplicate blocks in the UI.
        if self.protocol_ingress.has_native_tool_calls {
            tracing::debug!(
                target: "vex::protocol",
                %name,
                "suppressing tagged XML tool call — native tool_calls already received this turn"
            );
            return Vec::new();
        }

        let index = self.protocol_ingress.next_tagged_tool_block_index;
        self.protocol_ingress.next_tagged_tool_block_index = self
            .protocol_ingress
            .next_tagged_tool_block_index
            .saturating_add(1);

        let mut envelopes = self.open_provider_stream_block(
            index,
            ProviderContentBlock::ToolUse {
                id: format!("toolu_tagged_{index}"),
                name,
                input,
            },
        );
        envelopes.extend(self.close_provider_stream_block(index));
        envelopes
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
            envelopes.push(self.emit_signal(RuntimeSignal::UsageUpdated { usage }));
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
                reasoning_content,
                refusal,
                tool_calls,
            } = delta;

            tracing::trace!(
                target: "vex::protocol",
                choice_index = choice_index.unwrap_or_default(),
                content_state = text_field_state(content.as_deref()),
                content_bytes = content.as_ref().map(|text| text.len()).unwrap_or_default(),
                reasoning_state = text_field_state(reasoning_content.as_deref()),
                reasoning_bytes = reasoning_content
                    .as_ref()
                    .map(|text| text.len())
                    .unwrap_or_default(),
                refusal_state = text_field_state(refusal.as_deref()),
                tool_call_count = tool_calls.as_ref().map(|items| items.len()).unwrap_or_default(),
                finish_reason = finish_reason.as_deref().unwrap_or("none"),
                has_logprobs = logprobs.is_some(),
                "normalizing chat-compatible choice"
            );

            if let Some(content) = content {
                envelopes
                    .extend(self.apply_provider_stream_delta(0, ProviderBlockDelta::Text(content)));
            }

            if let Some(reasoning) = reasoning_content {
                envelopes.extend(
                    self.apply_provider_stream_delta(0, ProviderBlockDelta::Text(reasoning)),
                );
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

            if let Some(finish_reason) = finish_reason.as_deref() {
                crate::observability::set_span_attributes(
                    &tracing::Span::current(),
                    [KeyValue::new(
                        semconv::GEN_AI_RESPONSE_FINISH_REASONS,
                        finish_reason.to_string(),
                    )],
                );
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

        tracing::trace!(
            target: "vex::protocol",
            role = role.as_deref().unwrap_or("unknown"),
            "observed chat-compatible message start"
        );
        self.protocol_ingress.chat_compat_message_started = true;

        if role.is_some() {
            self.protocol_ingress.chat_compat_stream_terminated = false;
        }
        message_start_metadata(metadata)
            .map(|metadata| self.emit_provider_metadata(metadata))
            .unwrap_or_default()
    }

    fn apply_chat_compat_tool_delta(
        &mut self,
        choice_index: Option<usize>,
        tool_call: ChatCompatToolCallDelta,
    ) -> Vec<RuntimeEnvelope> {
        if self.protocol_ingress.chat_compat_stream_terminated {
            tracing::debug!(
                target: "vex::protocol",
                choice_index = choice_index.unwrap_or_default(),
                "ignoring late chat-compatible tool delta after stream termination"
            );

            return Vec::new();
        }

        let raw_index = tool_call.index.unwrap_or(0).min(MAX_TOOL_CALL_INDEX);
        let block_index = raw_index.saturating_add(1);
        let has_id = tool_call.id.as_ref().is_some_and(|id| !id.is_empty());
        let has_function = tool_call.function.is_some();
        let has_name = tool_call
            .function
            .as_ref()
            .and_then(|function| function.name.as_ref())
            .is_some_and(|name| !name.is_empty());
        let arguments_shape = tool_call
            .function
            .as_ref()
            .and_then(|function| function.arguments.as_ref())
            .map(chat_compat_arguments_shape)
            .unwrap_or("none");
        let arguments_bytes = tool_call
            .function
            .as_ref()
            .and_then(|function| function.arguments.as_ref())
            .map(chat_compat_arguments_bytes)
            .unwrap_or_default();
        let _ = (choice_index, tool_call.call_type);

        tracing::debug!(
            target: "vex::protocol",
            choice_index = choice_index.unwrap_or_default(),
            raw_index,
            block_index,
            has_id,
            has_function,
            has_name,
            arguments_shape,
            arguments_bytes,
            "processing chat-compatible tool delta"
        );

        let (
            start_block,
            partial_json,
            lifecycle_after,
            pending_argument_bytes,
            has_materialized_arguments,
        ) = {
            let state = self
                .protocol_ingress
                .chat_compat_tool_lifecycles
                .entry(block_index)
                .or_default();

            if state.lifecycle == ToolCallLifecycle::Closed {
                return Vec::new();
            }

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
                    match arguments {
                        ChatCompatFunctionArguments::String(arguments) => {
                            if let Some(value) = state.materialized_arguments.take()
                                && let Ok(serialized) = serde_json::to_string(&value)
                            {
                                state.pending_arguments.push_str(&serialized);
                            }
                            state.pending_arguments.push_str(&arguments);
                        }
                        ChatCompatFunctionArguments::Json(arguments) => {
                            if state.lifecycle == ToolCallLifecycle::Discovered
                                && state.pending_arguments.is_empty()
                            {
                                state.materialized_arguments = Some(arguments);
                            } else if let Ok(serialized) = serde_json::to_string(&arguments) {
                                if let Some(value) = state.materialized_arguments.take()
                                    && let Ok(materialized) = serde_json::to_string(&value)
                                {
                                    state.pending_arguments.push_str(&materialized);
                                }
                                state.pending_arguments.push_str(&serialized);
                            }
                        }
                    }
                }
            }

            let start_block = (state.lifecycle == ToolCallLifecycle::Discovered
                && !state.name.is_empty())
            .then(|| {
                let id = if state.id.is_empty() {
                    format!("toolu_chat_compat_{block_index}")
                } else {
                    state.id.clone()
                };
                state.lifecycle = ToolCallLifecycle::Opened;
                ProviderContentBlock::ToolUse {
                    id,
                    name: state.name.clone(),
                    input: state
                        .materialized_arguments
                        .take()
                        .unwrap_or_else(empty_json_object),
                }
            });

            let partial_json = (state.lifecycle == ToolCallLifecycle::Opened
                && !state.pending_arguments.is_empty())
            .then(|| std::mem::take(&mut state.pending_arguments));

            (
                start_block,
                partial_json,
                state.lifecycle,
                state.pending_arguments.len(),
                state.materialized_arguments.is_some(),
            )
        };

        tracing::debug!(
            target: "vex::protocol",
            choice_index = choice_index.unwrap_or_default(),
            block_index,
            lifecycle = tool_call_lifecycle_name(lifecycle_after),
            pending_argument_bytes,
            has_materialized_arguments,
            emitted_start_block = start_block.is_some(),
            emitted_partial_argument_bytes = partial_json.as_ref().map(|json| json.len()).unwrap_or_default(),
            "normalized chat-compatible tool delta"
        );

        let mut envelopes = Vec::new();
        if let Some(content_block) = start_block {
            tracing::debug!(
                target: "vex::protocol",
                block_index,
                "opening chat-compatible tool block"
            );
            // Mark that this turn has native tool calls so that XML tool calls synthesised
            // from text content are suppressed (prevents duplicate blocks when the model
            // echoes its tool call both in tool_calls and as XML inside content).
            self.protocol_ingress.has_native_tool_calls = true;
            envelopes.extend(self.open_provider_stream_block(block_index, content_block));
        }
        if let Some(partial_json) = partial_json {
            tracing::debug!(
                target: "vex::protocol",
                block_index,
                partial_bytes = partial_json.len(),
                "applying chat-compatible tool arguments delta"
            );
            envelopes.extend(self.apply_provider_stream_delta(
                block_index,
                ProviderBlockDelta::ToolArguments(partial_json),
            ));
        }
        envelopes
    }

    fn close_chat_compat_tool_blocks(&mut self) -> Vec<RuntimeEnvelope> {
        let to_close: Vec<usize> = self
            .protocol_ingress
            .chat_compat_tool_lifecycles
            .iter_mut()
            .filter_map(|(index, state)| match state.lifecycle {
                ToolCallLifecycle::Opened => {
                    state.lifecycle = ToolCallLifecycle::Closed;
                    Some(*index)
                }
                ToolCallLifecycle::Discovered => {
                    state.lifecycle = ToolCallLifecycle::Closed;
                    None
                }
                ToolCallLifecycle::Closed => None,
            })
            .collect();

        let mut envelopes = Vec::new();
        for index in to_close {
            tracing::debug!(
                target: "vex::protocol",
                index,
                "closing chat-compatible tool block"
            );
            envelopes.extend(self.close_provider_stream_block(index));
        }
        envelopes
    }
}

fn text_field_state(value: Option<&str>) -> &'static str {
    match value {
        Some("") => "empty",
        Some(_) => "text",
        None => "missing_or_null",
    }
}

fn chat_compat_arguments_shape(arguments: &ChatCompatFunctionArguments) -> &'static str {
    match arguments {
        ChatCompatFunctionArguments::String(_) => "string",
        ChatCompatFunctionArguments::Json(_) => "json",
    }
}

fn chat_compat_arguments_bytes(arguments: &ChatCompatFunctionArguments) -> usize {
    match arguments {
        ChatCompatFunctionArguments::String(arguments) => arguments.len(),
        ChatCompatFunctionArguments::Json(arguments) => arguments.to_string().len(),
    }
}

fn tool_call_lifecycle_name(lifecycle: ToolCallLifecycle) -> &'static str {
    match lifecycle {
        ToolCallLifecycle::Discovered => "discovered",
        ToolCallLifecycle::Opened => "opened",
        ToolCallLifecycle::Closed => "closed",
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

fn accumulate_turn_tokens(turn_tokens: &mut PulseTokens, usage: &ApiUsage) {
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
