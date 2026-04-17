use super::logging::emit_sse_parse_error;
use crate::types::{
    ApiStreamError, ApiUsage, ContentBlock, Delta, MessageDelta, MessageStartData,
    StreamChunkMetadata, StreamEvent, StreamPromptProgress, StreamTimings, ToolUseMetadata,
};
use anyhow::Result;
use serde::Deserialize;

mod mappers;
mod text_normaliser;

pub(crate) use self::mappers::{ProtocolMapper, SseFrame, mapper_for_variant};
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

#[derive(Default)]
pub struct StreamParser {
    buffer: Vec<u8>,
    chat_compat_tools: Vec<ChatCompatToolState>,
    chat_compat_message_started: bool,
    bom_checked: bool,
    overflowed: bool,
    last_event_id: Option<String>,
    retry_ms: Option<u64>,
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

    pub fn process_sse_event(&mut self, event_type: &str, data: &str) -> Result<Vec<StreamEvent>> {
        self.parse_event_payload((!event_type.is_empty()).then_some(event_type), data)
    }

    pub fn process(&mut self, chunk: &[u8]) -> Result<Vec<StreamEvent>> {
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

    fn parse_frame_bytes(&mut self, frame_bytes: Vec<u8>) -> Result<Vec<StreamEvent>> {
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
            } else if let Some(rest) = line.strip_prefix("id:") {
                self.last_event_id = Some(strip_single_leading_space(rest).to_string());
            } else if let Some(rest) = line.strip_prefix("retry:") {
                let value = strip_single_leading_space(rest);
                if value.bytes().all(|byte| byte.is_ascii_digit()) {
                    self.retry_ms = value.parse::<u64>().ok();
                }
            } else if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(strip_single_leading_space(rest).to_string());
            }
        }

        if data_lines.is_empty() {
            if frame_without_data_should_error(&normalised_frame) {
                return Ok(vec![StreamEvent::Error {
                    error: ApiStreamError {
                        error_type: "sse_parse_error".to_string(),
                        message: "received a non-SSE payload without a data field; raw JSON chunk streams are unsupported"
                            .to_string(),
                    },
                }]);
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
    ) -> Result<Vec<StreamEvent>> {
        if json_data.is_empty() {
            return Ok(Vec::new());
        }
        if event_type == Some("ping") {
            return Ok(vec![StreamEvent::Ping]);
        }

        match serde_json::from_str::<StreamEvent>(json_data) {
            Ok(evt) => Ok(vec![evt]),
            Err(messages_v1_error) => {
                if let Some(chat_compat_events) = self.parse_chat_compat_chunk(json_data) {
                    Ok(chat_compat_events)
                } else {
                    emit_sse_parse_error(event_type, json_data, &messages_v1_error);
                    // Emit a structured error event so the runtime can surface
                    // the failure to the UI rather than silently dropping the
                    // frame.  ADR-021 Item 19.
                    Ok(vec![StreamEvent::Error {
                        error: ApiStreamError {
                            error_type: "sse_parse_error".to_string(),
                            message: messages_v1_error.to_string(),
                        },
                    }])
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
        if self.buffer[..prefix_len] != UTF8_BOM[..prefix_len]
            || self.buffer.len() >= UTF8_BOM.len()
        {
            self.bom_checked = true;
        }
    }

    fn buffer_overflow_event(&self) -> StreamEvent {
        StreamEvent::Error {
            error: ApiStreamError {
                error_type: "sse_buffer_overflow".to_string(),
                message: format!(
                    "SSE intra-frame buffer exceeded {MAX_SSE_BUFFER_BYTES} bytes without a \
                     frame delimiter; the upstream stream may be malformed"
                ),
            },
        }
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
        .any(|line| !line.contains(':') || looks_like_raw_json_payload(line))
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
