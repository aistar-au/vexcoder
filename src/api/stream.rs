use super::logging::emit_sse_parse_error;
use crate::runtime::json_handoff::{RuntimeEnvelopeNormalizer, TurnEndContext};
use crate::runtime::{RuntimeEnvelope, RuntimeEvent, TokenUsageEnvelope, UiUpdate};
use crate::state::{StreamBlock, ToolStatus};
use crate::types::{
    ApiStreamError, ApiUsage, ContentBlock, Delta, MessageDelta, MessageStartData,
    StreamChunkMetadata, StreamEvent, StreamPromptProgress, StreamTimings, ToolUseMetadata,
};
use crate::usage::TurnTokens;
use anyhow::Result;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};

mod text_normaliser;

pub use self::text_normaliser::{NormalisedChunk, StreamTextNormaliser};

/// Maximum number of bytes the SSE intra-frame accumulation buffer may hold.
/// Chunks are drained as soon as a frame delimiter (`\n\n` or `\r\n\r\n`) is
/// found, so this bound is only reached when the upstream stream emits no
/// delimiters.  A 1 MiB ceiling matches the editor cap and is far above any
/// real SSE frame.  ADR-021 Item 26.
const MAX_SSE_BUFFER_BYTES: usize = 1_048_576;

/// Hard ceiling on the tool-call index accepted from a chat-compat stream.
/// Indices beyond this cap are clamped to prevent unbounded Vec allocation
/// from untrusted server data.
const MAX_TOOL_CALL_INDEX: usize = 1_024;

static NEXT_STREAM_TASK_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum StreamProtocolMode {
    #[default]
    Undecided,
    RuntimeEnvelope,
    ProviderNormalized,
}

#[derive(Default)]
pub struct StreamParser {
    buffer: Vec<u8>,
    chat_compat_tools: Vec<ChatCompatToolState>,
    chat_compat_message_started: bool,
    bom_checked: bool,
    overflowed: bool,
    // WHATWG HTML §9.2.6 — last received event ID and reconnection delay.
    // Updated as id: and retry: fields are parsed; exposed for callers that
    // implement reconnection with Last-Event-ID semantics.
    last_event_id: Option<String>,
    reconnect_delay_ms: Option<u64>,
    protocol_mode: StreamProtocolMode,
    provider_normalizer: Option<RuntimeEnvelopeNormalizer>,
    provider_turn_started: bool,
    provider_open_blocks: BTreeSet<usize>,
    provider_turn_tokens: TurnTokens,
}

#[derive(Default, Clone)]
struct ChatCompatToolState {
    id: String,
    name: String,
    pending_arguments: String,
    started: bool,
    stopped: bool,
}

// Chat-compat structs deserialize the full documented chat completions streaming surface.
// Fields that are not yet consumed by the conversion logic are retained so that
// serde does not silently drop documented values if future code needs them.
#[derive(Debug, Deserialize)]
struct ChatCompatChunk {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    object: Option<String>,
    #[serde(default)]
    created: Option<u64>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    system_fingerprint: Option<String>,
    #[serde(default)]
    service_tier: Option<String>,
    #[serde(default)]
    prompt_progress: Option<StreamPromptProgress>,
    #[serde(default)]
    timings: Option<StreamTimings>,
    #[serde(default)]
    choices: Vec<ChatCompatChoice>,
    #[serde(default)]
    usage: Option<ApiUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatCompatChoice {
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    delta: ChatCompatDelta,
    #[serde(default)]
    finish_reason: Option<String>,
    #[serde(default)]
    logprobs: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct ChatCompatDelta {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    refusal: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ChatCompatToolCallDelta>>,
}

#[derive(Debug, Default, Deserialize)]
struct ChatCompatToolCallDelta {
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "type", default)]
    call_type: Option<String>,
    #[serde(default)]
    function: Option<ChatCompatFunctionDelta>,
}

#[derive(Debug, Default, Deserialize)]
struct ChatCompatFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

impl StreamParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process_sse_event(
        &mut self,
        event_type: &str,
        data: &str,
    ) -> Result<Vec<RuntimeEnvelope>> {
        self.parse_event_payload((!event_type.is_empty()).then_some(event_type), data)
    }

    pub fn process(&mut self, chunk: &[u8]) -> Result<Vec<RuntimeEnvelope>> {
        if self.overflowed {
            return Ok(vec![self.buffer_overflow_event()]);
        }

        if self.buffer.len().saturating_add(chunk.len()) > MAX_SSE_BUFFER_BYTES {
            self.overflowed = true;
            return Ok(vec![self.buffer_overflow_event()]);
        }
        self.buffer.extend_from_slice(chunk);
        self.strip_utf8_bom_once();

        let mut events = Vec::new();

        while let Some((pos, delim_len)) = self.find_delimiter() {
            let end = pos + delim_len;
            let frame_bytes = self.buffer[..pos].to_vec();
            self.buffer.drain(..end);
            events.extend(self.parse_frame_bytes(frame_bytes)?);
        }

        Ok(events)
    }

    fn parse_frame_bytes(&mut self, frame_bytes: Vec<u8>) -> Result<Vec<RuntimeEnvelope>> {
        let frame_text = String::from_utf8(frame_bytes)?;
        let normalised_frame = normalise_sse_line_endings(&frame_text);
        let mut event_type = None;
        let mut data_lines = Vec::new();

        for line in normalised_frame.split('\n') {
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("event:") {
                event_type = Some(strip_single_leading_space(rest).to_string());
            } else if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(strip_single_leading_space(rest).to_string());
            } else if let Some(rest) = line.strip_prefix("id:") {
                let val = strip_single_leading_space(rest);
                if !val.contains('\0') {
                    self.last_event_id = Some(val.to_string());
                }
            } else if line == "id" {
                self.last_event_id = Some(String::new());
            } else if let Some(rest) = line.strip_prefix("retry:")
                && let Ok(ms) = strip_single_leading_space(rest).parse::<u64>()
            {
                self.reconnect_delay_ms = Some(ms);
            }
        }

        if data_lines.is_empty() {
            if frame_without_data_should_error(&normalised_frame) {
                return Ok(vec![self.provider_error_envelope(
                    "sse_parse_error",
                    "received a non-SSE payload without a data field; raw JSON chunk streams are unsupported"
                        .to_string(),
                )]);
            }
            return Ok(Vec::new());
        }

        let json_data = data_lines.join("\n");

        self.parse_event_payload(event_type.as_deref(), &json_data)
    }

    fn parse_event_payload(
        &mut self,
        event_type: Option<&str>,
        json_data: &str,
    ) -> Result<Vec<RuntimeEnvelope>> {
        if json_data.is_empty() {
            return Ok(Vec::new());
        }

        if let Ok(envelope) = serde_json::from_str::<RuntimeEnvelope>(json_data) {
            self.protocol_mode = StreamProtocolMode::RuntimeEnvelope;
            return Ok(vec![envelope]);
        }

        let events = self.parse_legacy_event_payload(event_type, json_data);
        Ok(self.normalize_provider_events(events))
    }

    fn parse_legacy_event_payload(
        &mut self,
        event_type: Option<&str>,
        json_data: &str,
    ) -> Vec<StreamEvent> {
        if event_type == Some("ping") {
            return vec![StreamEvent::Ping];
        }

        match serde_json::from_str::<StreamEvent>(json_data) {
            Ok(evt) => vec![evt],
            Err(messages_v1_error) => {
                if let Some(chat_compat_events) = self.parse_chat_compat_chunk(json_data) {
                    chat_compat_events
                } else {
                    emit_sse_parse_error(event_type, json_data, &messages_v1_error);
                    // Emit a structured error event so the runtime can surface
                    // the failure to the UI rather than silently dropping the
                    // frame.  ADR-021 Item 19.
                    vec![StreamEvent::Error {
                        error: ApiStreamError {
                            error_type: "sse_parse_error".to_string(),
                            message: messages_v1_error.to_string(),
                        },
                    }]
                }
            }
        }
    }

    fn find_delimiter(&self) -> Option<(usize, usize)> {
        let mut index = 0;
        while index < self.buffer.len() {
            let Some(first_len) = line_terminator_len(&self.buffer, index) else {
                index += 1;
                continue;
            };
            let next = index + first_len;
            if let Some(second_len) = line_terminator_len(&self.buffer, next) {
                return Some((index, first_len + second_len));
            }
            index = next;
        }
        None
    }

    fn strip_utf8_bom_once(&mut self) {
        if self.bom_checked {
            return;
        }

        const UTF8_BOM: &[u8; 3] = b"\xEF\xBB\xBF";
        if self.buffer.starts_with(UTF8_BOM) {
            self.buffer.drain(..UTF8_BOM.len());
            self.bom_checked = true;
            return;
        }

        if self.buffer.is_empty() {
            return;
        }

        let prefix_len = self.buffer.len().min(UTF8_BOM.len());
        if prefix_len == UTF8_BOM.len() || self.buffer[..prefix_len] != UTF8_BOM[..prefix_len] {
            self.bom_checked = true;
        }
    }

    fn buffer_overflow_event(&mut self) -> RuntimeEnvelope {
        self.provider_error_envelope(
            "sse_buffer_overflow",
            format!(
                "SSE intra-frame buffer exceeded {MAX_SSE_BUFFER_BYTES} bytes without a \
                 frame delimiter; the upstream stream may be malformed"
            ),
        )
    }

    fn parse_chat_compat_chunk(&mut self, json_data: &str) -> Option<Vec<StreamEvent>> {
        if json_data == "[DONE]" {
            let mut events = Vec::new();
            self.close_chat_compat_tool_blocks(&mut events);
            self.chat_compat_message_started = false;
            self.chat_compat_tools.clear();
            return Some(events);
        }

        let mut chunk = serde_json::from_str::<ChatCompatChunk>(json_data).ok()?;
        let mut events = Vec::new();
        self.emit_chat_compat_message_start(&chunk, &mut events);

        if let Some(mut usage) = chunk.usage.take() {
            if usage.service_tier.is_none() {
                usage.service_tier = chunk.service_tier.clone();
            }
            events.push(StreamEvent::MessageDelta {
                delta: MessageDelta {
                    stop_reason: None,
                    stop_sequence: None,
                    role: None,
                    refusal: None,
                    metadata: self.chat_compat_metadata(&chunk, None, None),
                },
                usage: Some(usage),
            });
        }

        if chunk.choices.is_empty() {
            return Some(events);
        }

        let chunk_object = chunk.object.clone();
        let chunk_created = chunk.created;
        let chunk_system_fingerprint = chunk.system_fingerprint.clone();
        let chunk_service_tier = chunk.service_tier.clone();
        let chunk_prompt_progress = chunk.prompt_progress.clone();
        let chunk_timings = chunk.timings.clone();
        let metadata_for_choice =
            |choice_index: Option<usize>, logprobs: Option<serde_json::Value>| {
                let metadata = StreamChunkMetadata {
                    object: chunk_object.clone(),
                    created: chunk_created,
                    system_fingerprint: chunk_system_fingerprint.clone(),
                    service_tier: chunk_service_tier.clone(),
                    choice_index,
                    logprobs,
                    prompt_progress: chunk_prompt_progress.clone(),
                    timings: chunk_timings.clone(),
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
            };

        for choice in chunk.choices {
            let ChatCompatChoice {
                index: choice_index,
                delta,
                finish_reason,
                logprobs,
            } = choice;
            let ChatCompatDelta {
                role,
                content,
                refusal,
                tool_calls,
            } = delta;

            if let Some(content) = content {
                events.push(StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: Delta {
                        delta_type: Some("text_delta".to_string()),
                        text: Some(content),
                        partial_json: None,
                        thinking: None,
                        signature: None,
                        choice_index,
                    },
                });
            }

            if let Some(tool_calls) = tool_calls {
                for tool_call in tool_calls {
                    self.apply_chat_compat_tool_delta(choice_index, tool_call, &mut events);
                }
            }

            let has_logprobs = logprobs.is_some();
            let metadata = metadata_for_choice(choice_index, logprobs);
            let has_server_progress = metadata
                .as_ref()
                .is_some_and(|md| md.prompt_progress.is_some() || md.timings.is_some());

            if refusal.is_some() || finish_reason.is_some() || has_logprobs || has_server_progress {
                events.push(StreamEvent::MessageDelta {
                    delta: MessageDelta {
                        stop_reason: finish_reason.clone(),
                        stop_sequence: None,
                        role,
                        refusal,
                        metadata,
                    },
                    usage: None,
                });
            }

            if finish_reason.is_some() {
                self.close_chat_compat_tool_blocks(&mut events);
            }
        }

        Some(events)
    }

    fn emit_chat_compat_message_start(
        &mut self,
        chunk: &ChatCompatChunk,
        events: &mut Vec<StreamEvent>,
    ) {
        if self.chat_compat_message_started {
            return;
        }

        let role = chunk
            .choices
            .iter()
            .find_map(|choice| choice.delta.role.clone());
        let metadata = self.chat_compat_metadata(chunk, None, None);
        let has_message_fields =
            chunk.id.is_some() || chunk.model.is_some() || role.is_some() || metadata.is_some();
        if !has_message_fields {
            return;
        }

        events.push(StreamEvent::MessageStart {
            message: MessageStartData {
                id: chunk.id.clone().unwrap_or_default(),
                message_type: None,
                role: role.unwrap_or_else(|| "assistant".to_string()),
                model: chunk.model.clone().unwrap_or_default(),
                content: None,
                stop_reason: None,
                stop_sequence: None,
                usage: None,
                metadata,
            },
        });
        self.chat_compat_message_started = true;
    }

    fn chat_compat_metadata(
        &self,
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

    fn apply_chat_compat_tool_delta(
        &mut self,
        choice_index: Option<usize>,
        tool_call: ChatCompatToolCallDelta,
        events: &mut Vec<StreamEvent>,
    ) {
        let raw_index = tool_call.index.unwrap_or(0).min(MAX_TOOL_CALL_INDEX);
        let block_index = raw_index.saturating_add(1);
        self.ensure_chat_compat_tool_state(block_index);
        let state = &mut self.chat_compat_tools[block_index];
        let call_type = tool_call.call_type.clone();

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

            events.push(StreamEvent::ContentBlockStart {
                index: block_index,
                content_block: ContentBlock::ToolUse {
                    id,
                    name: state.name.clone(),
                    input: serde_json::Value::Object(serde_json::Map::new()),
                    metadata: Some(ToolUseMetadata {
                        call_type,
                        choice_index,
                    })
                    .filter(|metadata| {
                        metadata.call_type.is_some() || metadata.choice_index.is_some()
                    }),
                },
            });
            state.started = true;
        }

        if state.started && !state.pending_arguments.is_empty() {
            let partial_json = std::mem::take(&mut state.pending_arguments);
            events.push(StreamEvent::ContentBlockDelta {
                index: block_index,
                delta: Delta {
                    delta_type: Some("input_json_delta".to_string()),
                    text: None,
                    partial_json: Some(partial_json),
                    thinking: None,
                    signature: None,
                    choice_index,
                },
            });
        }
    }

    fn ensure_chat_compat_tool_state(&mut self, index: usize) {
        let required = index.saturating_add(1);
        if self.chat_compat_tools.len() < required {
            self.chat_compat_tools
                .resize_with(required, ChatCompatToolState::default);
        }
    }

    fn close_chat_compat_tool_blocks(&mut self, events: &mut Vec<StreamEvent>) {
        for (index, state) in self.chat_compat_tools.iter_mut().enumerate() {
            if index == 0 {
                continue;
            }
            if state.started && !state.stopped {
                events.push(StreamEvent::ContentBlockStop { index });
                state.stopped = true;
            }
        }
    }

    /// Returns the last event ID received from the stream (WHATWG HTML §9.2.6).
    pub fn last_event_id(&self) -> Option<&str> {
        self.last_event_id.as_deref()
    }

    /// Returns the reconnection delay hint from the stream in milliseconds (WHATWG HTML §9.2.6).
    pub fn reconnect_delay_ms(&self) -> Option<u64> {
        self.reconnect_delay_ms
    }

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

    fn normalize_provider_events(&mut self, events: Vec<StreamEvent>) -> Vec<RuntimeEnvelope> {
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

    fn normalize_provider_event(&mut self, event: StreamEvent) -> Vec<RuntimeEnvelope> {
        match event {
            StreamEvent::MessageStart { message } => {
                let mut envelopes = Vec::new();
                if let Some(metadata) = message_start_metadata(message.metadata) {
                    envelopes.extend(self.emit_server_metadata(metadata));
                }
                if let Some(usage) = message.usage {
                    envelopes.extend(self.emit_usage_update(usage));
                }
                envelopes
            }
            StreamEvent::ContentBlockStart {
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
            StreamEvent::ContentBlockDelta { index, delta } => {
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
            StreamEvent::ContentBlockStop { index } => {
                if !self.provider_open_blocks.remove(&index) {
                    return Vec::new();
                }
                self.provider_normalizer_mut()
                    .normalize_ui_update(&UiUpdate::StreamBlockComplete { index }, None)
            }
            StreamEvent::MessageDelta { delta, usage } => {
                let mut envelopes = Vec::new();
                if let Some(metadata) = delta.metadata {
                    envelopes.extend(self.emit_server_metadata(metadata));
                }
                if let Some(usage) = usage {
                    envelopes.extend(self.emit_usage_update(usage));
                }
                envelopes
            }
            StreamEvent::MessageStop | StreamEvent::Ping | StreamEvent::Unknown => Vec::new(),
            StreamEvent::Error { error } => {
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

    fn provider_error_envelope(&mut self, code: &str, message: String) -> RuntimeEnvelope {
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

fn next_stream_task_id() -> String {
    let id = NEXT_STREAM_TASK_ID.fetch_add(1, Ordering::Relaxed);
    format!("api_stream_{id}")
}

fn provider_stream_block(content_block: &ContentBlock) -> Option<(StreamBlock, Option<String>)> {
    match content_block {
        ContentBlock::Text { text, .. } => Some((
            StreamBlock::Thinking {
                content: String::new(),
                collapsed: false,
            },
            Some(text.clone()),
        )),
        ContentBlock::Thinking { thinking, .. } => Some((
            StreamBlock::Thinking {
                content: String::new(),
                collapsed: false,
            },
            Some(thinking.clone()),
        )),
        ContentBlock::ThinkingData { data } => Some((
            StreamBlock::Thinking {
                content: String::new(),
                collapsed: false,
            },
            Some(data.clone()),
        )),
        ContentBlock::ToolUse {
            id, name, input, ..
        }
        | ContentBlock::ServerToolUse { id, name, input } => Some((
            StreamBlock::ToolCall {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
                status: ToolStatus::Pending,
            },
            None,
        )),
        ContentBlock::ToolResult { .. } | ContentBlock::WebSearchToolResult { .. } => None,
    }
}

fn provider_block_delta(delta: &Delta) -> Option<ProviderBlockDelta> {
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

fn strip_single_leading_space(value: &str) -> &str {
    value.strip_prefix(' ').unwrap_or(value)
}

fn normalise_sse_line_endings(frame: &str) -> String {
    frame.replace("\r\n", "\n").replace('\r', "\n")
}

fn frame_without_data_should_error(frame: &str) -> bool {
    frame
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(':'))
        .any(looks_like_raw_json_payload)
}

fn looks_like_raw_json_payload(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && (trimmed == "[DONE]"
            || ((trimmed.starts_with('{') || trimmed.starts_with('['))
                && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()))
}

fn line_terminator_len(buffer: &[u8], index: usize) -> Option<usize> {
    match buffer.get(index) {
        Some(b'\n') => Some(1),
        Some(b'\r') => {
            if buffer.get(index + 1) == Some(&b'\n') {
                Some(2)
            } else {
                Some(1)
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
