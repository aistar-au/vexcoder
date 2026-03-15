use super::logging::emit_sse_parse_error;
use crate::types::{ApiUsage, ContentBlock, Delta, MessageDelta, StreamEvent};
use anyhow::Result;
use serde::Deserialize;

#[derive(Default)]
pub struct StreamParser {
    buffer: Vec<u8>,
    chat_compat_tools: Vec<ChatCompatToolState>,
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
        const MAX_BUFFER_SIZE: usize = 1024 * 1024; // 1MB limit
        if self.buffer.len() + chunk.len() > MAX_BUFFER_SIZE {
            anyhow::bail!("Stream buffer limit exceeded");
        }
        self.buffer.extend_from_slice(chunk);

        let mut events = Vec::new();

        while let Some((pos, delim_len)) = self.find_delimiter() {
            let end = pos + delim_len;
            let frame_bytes = self.buffer[..pos].to_vec();
            self.buffer.drain(..end);

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

            if !data_lines.is_empty() {
                let json_data = data_lines.join("\n");
                if event_type.as_deref() == Some("ping") {
                    events.push(StreamEvent::Ping);
                    continue;
                }

                match serde_json::from_str::<StreamEvent>(&json_data) {
                    Ok(evt) => events.push(evt),
                    Err(messages_v1_error) => {
                        if let Some(chat_compat_events) = self.parse_chat_compat_chunk(&json_data) {
                            events.extend(chat_compat_events);
                        } else {
                            emit_sse_parse_error(
                                event_type.as_deref(),
                                &json_data,
                                &messages_v1_error,
                            );
                        }
                    }
                }
            }
        }

        Ok(events)
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
            return Some(events);
        }

        let chunk = serde_json::from_str::<ChatCompatChunk>(json_data).ok()?;
        let mut events = Vec::new();

        if let Some(usage) = chunk.usage {
            events.push(StreamEvent::MessageDelta {
                delta: MessageDelta {
                    stop_reason: None,
                    stop_sequence: None,
                },
                usage: Some(usage),
            });
        }

        if chunk.choices.is_empty() {
            return Some(events);
        }

        for choice in chunk.choices {
            if let Some(content) = choice.delta.content {
                events.push(StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: Delta {
                        delta_type: Some("text_delta".to_string()),
                        text: Some(content),
                        partial_json: None,
                        thinking: None,
                        signature: None,
                    },
                });
            }

            if let Some(tool_calls) = choice.delta.tool_calls {
                for tool_call in tool_calls {
                    self.apply_chat_compat_tool_delta(tool_call, &mut events);
                }
            }

            if choice.finish_reason.is_some() {
                self.close_chat_compat_tool_blocks(&mut events);
            }
        }

        Some(events)
    }

    fn apply_chat_compat_tool_delta(
        &mut self,
        tool_call: ChatCompatToolCallDelta,
        events: &mut Vec<StreamEvent>,
    ) {
        let block_index = tool_call.index.unwrap_or(0) + 1;
        self.ensure_chat_compat_tool_state(block_index);
        let state = &mut self.chat_compat_tools[block_index];

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

#[cfg(test)]
mod tests {
    use super::StreamParser;
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
    fn test_process_anthropic_message_delta_top_level_usage() {
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
}
