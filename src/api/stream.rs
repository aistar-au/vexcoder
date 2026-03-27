use super::logging::emit_sse_parse_error;
use crate::types::{
    ApiStreamError, ApiUsage, ContentBlock, Delta, MessageDelta, MessageStartData,
    StreamChunkMetadata, StreamEvent, ToolUseMetadata,
};
use anyhow::Result;
use serde::Deserialize;

/// Maximum number of bytes the SSE intra-frame accumulation buffer may hold.
/// Chunks are drained as soon as a frame delimiter (`\n\n` or `\r\n\r\n`) is
/// found, so this bound is only reached when the upstream stream emits no
/// delimiters.  A 1 MiB ceiling matches the editor cap and is far above any
/// real SSE frame.  ADR-021 Item 26.
const MAX_SSE_BUFFER_BYTES: usize = 1_048_576;

#[derive(Default)]
pub struct StreamParser {
    buffer: Vec<u8>,
    chat_compat_tools: Vec<ChatCompatToolState>,
    chat_compat_message_started: bool,
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
#[allow(dead_code)]
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
    choices: Vec<ChatCompatChoice>,
    #[serde(default)]
    usage: Option<ApiUsage>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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

    pub fn process(&mut self, chunk: &[u8]) -> Result<Vec<StreamEvent>> {
        if self.buffer.len() + chunk.len() > MAX_SSE_BUFFER_BYTES {
            return Ok(vec![StreamEvent::Error {
                error: ApiStreamError {
                    error_type: "sse_buffer_overflow".to_string(),
                    message: format!(
                        "SSE intra-frame buffer exceeded {MAX_SSE_BUFFER_BYTES} bytes without a \
                         frame delimiter; the upstream stream may be malformed"
                    ),
                },
            }]);
        }
        self.buffer.extend_from_slice(chunk);

        let mut events = Vec::new();

        while let Some((pos, delim_len)) = self.find_delimiter() {
            let end = pos + delim_len;
            let frame_bytes = self.buffer[..pos].to_vec();
            self.buffer.drain(..end);
            events.extend(self.parse_frame_bytes(frame_bytes)?);
        }

        if self.find_delimiter().is_none() {
            let frame_text = String::from_utf8_lossy(&self.buffer).trim().to_string();
            if looks_like_raw_json_frame(&frame_text) {
                let frame_bytes = std::mem::take(&mut self.buffer);
                events.extend(self.parse_frame_bytes(frame_bytes)?);
            }
        }

        Ok(events)
    }

    fn parse_frame_bytes(&mut self, frame_bytes: Vec<u8>) -> Result<Vec<StreamEvent>> {
        let frame_text = String::from_utf8(frame_bytes)?;
        let mut event_type = None;
        let mut data_lines = Vec::new();

        for line in frame_text.lines() {
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("event:") {
                event_type = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(rest.trim_start().to_string());
            }
        }

        let json_data = if !data_lines.is_empty() {
            data_lines.join("\n")
        } else {
            frame_text.trim().to_string()
        };

        if json_data.is_empty() {
            return Ok(Vec::new());
        }
        if event_type.as_deref() == Some("ping") {
            return Ok(vec![StreamEvent::Ping]);
        }

        match serde_json::from_str::<StreamEvent>(&json_data) {
            Ok(evt) => Ok(vec![evt]),
            Err(messages_v1_error) => {
                if let Some(chat_compat_events) = self.parse_chat_compat_chunk(&json_data) {
                    Ok(chat_compat_events)
                } else {
                    emit_sse_parse_error(event_type.as_deref(), &json_data, &messages_v1_error);
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
        if let Some(pos) = self.buffer.windows(2).position(|w| w == b"\n\n") {
            return Some((pos, 2));
        }
        if let Some(pos) = self.buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            return Some((pos, 4));
        }
        None
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
                    metadata: self.chat_compat_metadata(
                        chunk.object.clone(),
                        chunk.created,
                        chunk.system_fingerprint.clone(),
                        chunk.service_tier.clone(),
                        None,
                        None,
                    ),
                },
                usage: Some(usage),
            });
        }

        if chunk.choices.is_empty() {
            return Some(events);
        }

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

            if refusal.is_some() || finish_reason.is_some() || logprobs.is_some() {
                events.push(StreamEvent::MessageDelta {
                    delta: MessageDelta {
                        stop_reason: finish_reason.clone(),
                        stop_sequence: None,
                        role,
                        refusal,
                        metadata: self.chat_compat_metadata(
                            chunk.object.clone(),
                            chunk.created,
                            chunk.system_fingerprint.clone(),
                            chunk.service_tier.clone(),
                            choice_index,
                            logprobs,
                        ),
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
        let metadata = self.chat_compat_metadata(
            chunk.object.clone(),
            chunk.created,
            chunk.system_fingerprint.clone(),
            chunk.service_tier.clone(),
            None,
            None,
        );
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
        object: Option<String>,
        created: Option<u64>,
        system_fingerprint: Option<String>,
        service_tier: Option<String>,
        choice_index: Option<usize>,
        logprobs: Option<serde_json::Value>,
    ) -> Option<StreamChunkMetadata> {
        let metadata = StreamChunkMetadata {
            object,
            created,
            system_fingerprint,
            service_tier,
            choice_index,
            logprobs,
        };
        (metadata.object.is_some()
            || metadata.created.is_some()
            || metadata.system_fingerprint.is_some()
            || metadata.service_tier.is_some()
            || metadata.choice_index.is_some()
            || metadata.logprobs.is_some())
        .then_some(metadata)
    }

    fn apply_chat_compat_tool_delta(
        &mut self,
        choice_index: Option<usize>,
        tool_call: ChatCompatToolCallDelta,
        events: &mut Vec<StreamEvent>,
    ) {
        let block_index = tool_call.index.unwrap_or(0) + 1;
        self.ensure_chat_compat_tool_state(block_index);
        let state = &mut self.chat_compat_tools[block_index];
        let call_type = tool_call.call_type.clone();

        if let Some(id) = tool_call.id {
            if !id.is_empty() {
                state.id = id;
            }
        }
        if let Some(function) = tool_call.function {
            if let Some(name) = function.name {
                if !name.is_empty() {
                    state.name = name;
                }
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
        if self.chat_compat_tools.len() <= index {
            self.chat_compat_tools
                .resize_with(index + 1, ChatCompatToolState::default);
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

fn looks_like_raw_json_frame(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && (trimmed.starts_with('{') || trimmed == "[DONE]")
        && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
}

#[cfg(test)]
mod tests {
    use super::{StreamParser, MAX_SSE_BUFFER_BYTES};
    use crate::types::StreamEvent;

    #[test]
    fn test_process_emits_ping_for_ping_frame() {
        let mut parser = StreamParser::new();
        let events = parser
            .process(b"event: ping\ndata: {\"type\":\"ping\"}\n\n")
            .unwrap();

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], StreamEvent::Ping));
    }

    #[test]
    fn test_process_maps_chat_compat_usage_chunk() {
        let mut parser = StreamParser::new();
        let events = parser
            .process(
                b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":7,\"total_tokens\":19}}\n\n",
            )
            .unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::MessageDelta { usage, .. } => {
                let usage = usage.as_ref().expect("usage should be present");
                assert_eq!(usage.input_tokens, Some(12));
                assert_eq!(usage.output_tokens, Some(7));
                assert_eq!(usage.total_tokens, Some(19));
            }
            other => panic!("expected MessageDelta event, got {other:?}"),
        }
    }

    #[test]
    fn test_process_messages_v1_message_delta_top_level_usage() {
        let mut parser = StreamParser::new();
        let events = parser
            .process(
                b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":15}}\n\n",
            )
            .unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::MessageDelta { delta, usage } => {
                assert_eq!(delta.stop_reason.as_deref(), Some("end_turn"));
                let usage = usage.as_ref().expect("top-level usage must be present");
                assert_eq!(usage.output_tokens, Some(15));
            }
            other => panic!("expected MessageDelta event, got {other:?}"),
        }
    }

    #[test]
    fn test_process_chat_compat_emits_message_start_metadata() {
        let mut parser = StreamParser::new();
        let events = parser
            .process(
                br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","created":1741730100,"model":"model-name","system_fingerprint":"fp_123","service_tier":"standard","choices":[{"index":0,"delta":{"role":"assistant","content":"hello"},"finish_reason":null}]}

"#,
            )
            .unwrap();

        assert_eq!(events.len(), 2);
        match &events[0] {
            StreamEvent::MessageStart { message } => {
                assert_eq!(message.id, "chatcmpl-1");
                assert_eq!(message.role, "assistant");
                assert_eq!(message.model, "model-name");
                let metadata = message
                    .metadata
                    .as_ref()
                    .expect("metadata should be present");
                assert_eq!(metadata.object.as_deref(), Some("chat.completion.chunk"));
                assert_eq!(metadata.created, Some(1741730100));
                assert_eq!(metadata.system_fingerprint.as_deref(), Some("fp_123"));
                assert_eq!(metadata.service_tier.as_deref(), Some("standard"));
            }
            other => panic!("expected MessageStart event, got {other:?}"),
        }
    }

    #[test]
    fn test_process_chat_compat_emits_refusal_logprobs_and_choice_index() {
        let mut parser = StreamParser::new();
        let events = parser
            .process(
                br#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":2,"delta":{"role":"assistant","refusal":"cannot comply"},"logprobs":{"content":[]},"finish_reason":"stop"}]}

"#,
            )
            .unwrap();

        assert_eq!(events.len(), 2);
        match &events[1] {
            StreamEvent::MessageDelta { delta, usage } => {
                assert!(usage.is_none());
                assert_eq!(delta.stop_reason.as_deref(), Some("stop"));
                assert_eq!(delta.role.as_deref(), Some("assistant"));
                assert_eq!(delta.refusal.as_deref(), Some("cannot comply"));
                let metadata = delta.metadata.as_ref().expect("metadata should be present");
                assert_eq!(metadata.choice_index, Some(2));
                assert!(metadata.logprobs.is_some());
            }
            other => panic!("expected MessageDelta event, got {other:?}"),
        }
    }

    #[test]
    fn test_process_accepts_raw_json_frame_without_sse_delimiter() {
        let mut parser = StreamParser::new();
        let events = parser
            .process(
                br#"{"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"hello"},"finish_reason":"stop"}]}"#,
            )
            .unwrap();

        assert_eq!(events.len(), 3);
        assert!(matches!(events[0], StreamEvent::MessageStart { .. }));
        assert!(matches!(events[1], StreamEvent::ContentBlockDelta { .. }));
        assert!(matches!(events[2], StreamEvent::MessageDelta { .. }));
    }

    // -- ADR-021 Item 19: parse failures surface as StreamEvent::Error --------

    #[test]
    fn test_process_emits_error_event_on_unparseable_frame() {
        let mut parser = StreamParser::new();
        // A frame that is valid JSON but matches neither StreamEvent nor ChatCompatChunk.
        let events = parser
            .process(b"data: {\"totally\":\"unknown\"}\n\n")
            .unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Error { error } => {
                assert_eq!(error.error_type, "sse_parse_error");
            }
            other => panic!("expected StreamEvent::Error, got {other:?}"),
        }
    }

    // -- ADR-021 Item 26: buffer overflow surfaces as StreamEvent::Error ------

    #[test]
    fn test_process_emits_error_event_on_buffer_overflow() {
        let mut parser = StreamParser::new();
        // Send MAX_SSE_BUFFER_BYTES bytes with no delimiter so the buffer fills,
        // then send one more byte to trigger the overflow guard.
        let big_chunk = vec![b'x'; MAX_SSE_BUFFER_BYTES];
        let events = parser.process(&big_chunk).unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Error { error } => {
                assert_eq!(error.error_type, "sse_buffer_overflow");
            }
            other => panic!("expected StreamEvent::Error on overflow, got {other:?}"),
        }
    }
}
