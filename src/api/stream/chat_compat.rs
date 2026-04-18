use super::provider::{
    ProviderContentBlock, ProviderDelta, ProviderMessageDelta, ProviderMessageStartData,
    ProviderStreamEvent,
};
use super::{ChatCompatToolState, MAX_TOOL_CALL_INDEX, StreamParser};
use crate::types::{ApiUsage, StreamChunkMetadata, StreamPromptProgress, StreamTimings};
use serde::Deserialize;

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
    pub(super) fn parse_chat_compat_chunk(
        &mut self,
        json_data: &str,
    ) -> Option<Vec<ProviderStreamEvent>> {
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
            events.push(ProviderStreamEvent::MessageDelta {
                delta: ProviderMessageDelta {
                    _stop_reason: None,
                    _stop_sequence: None,
                    _role: None,
                    _refusal: None,
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
                events.push(ProviderStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ProviderDelta {
                        _delta_type: Some("text_delta".to_string()),
                        text: Some(content),
                        partial_json: None,
                        thinking: None,
                        _signature: None,
                        _choice_index: choice_index,
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
                events.push(ProviderStreamEvent::MessageDelta {
                    delta: ProviderMessageDelta {
                        _stop_reason: finish_reason.clone(),
                        _stop_sequence: None,
                        _role: role,
                        _refusal: refusal,
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
        events: &mut Vec<ProviderStreamEvent>,
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

        events.push(ProviderStreamEvent::MessageStart {
            message: ProviderMessageStartData {
                _id: chunk.id.clone().unwrap_or_default(),
                _message_type: None,
                _role: role.unwrap_or_else(|| "assistant".to_string()),
                _model: chunk.model.clone().unwrap_or_default(),
                _content: None,
                _stop_reason: None,
                _stop_sequence: None,
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
        events: &mut Vec<ProviderStreamEvent>,
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

            events.push(ProviderStreamEvent::ContentBlockStart {
                index: block_index,
                content_block: ProviderContentBlock::ToolUse {
                    id,
                    name: state.name.clone(),
                    input: serde_json::Value::Object(serde_json::Map::new()),
                },
            });
            state.started = true;
        }

        let _ = (call_type, choice_index);

        if state.started && !state.pending_arguments.is_empty() {
            let partial_json = std::mem::take(&mut state.pending_arguments);
            events.push(ProviderStreamEvent::ContentBlockDelta {
                index: block_index,
                delta: ProviderDelta {
                    _delta_type: Some("input_json_delta".to_string()),
                    text: None,
                    partial_json: Some(partial_json),
                    thinking: None,
                    _signature: None,
                    _choice_index: choice_index,
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

    fn close_chat_compat_tool_blocks(&mut self, events: &mut Vec<ProviderStreamEvent>) {
        for (index, state) in self.chat_compat_tools.iter_mut().enumerate() {
            if index == 0 {
                continue;
            }
            if state.started && !state.stopped {
                events.push(ProviderStreamEvent::ContentBlockStop { index });
                state.stopped = true;
            }
        }
    }
}
