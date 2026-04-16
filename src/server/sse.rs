use axum::response::sse::{Event, KeepAlive, Sse};
use chrono::Utc;
use futures::{Stream, StreamExt, stream};
use serde_json::json;
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

use super::SSE_KEEPALIVE_TEXT;
use crate::api::stream::{ProtocolMapper, SseFrame, mapper_for_variant};
use crate::config::ProtocolVariant;
use crate::runtime::json_handoff::{RuntimeEnvelope, RuntimeEvent, TokenUsageEnvelope};
use crate::state::StreamBlock;
use crate::types::ContentBlock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnsSseMode {
    RuntimeEnvelope,
    BlockDelta,
    ChoicesDelta,
}

#[derive(Debug)]
struct PendingToolBlock {
    block_index: usize,
    fallback_arguments_json: Option<String>,
}

#[derive(Debug)]
struct ActiveToolBlock {
    wire_index: usize,
    fallback_arguments_json: Option<String>,
    saw_delta: bool,
}

struct NegotiatedTurnsAdapter {
    mode: TurnsSseMode,
    mapper: Box<dyn ProtocolMapper>,
    message_id: Option<String>,
    active_text_blocks: HashSet<usize>,
    pending_tool_blocks: VecDeque<PendingToolBlock>,
    active_tool_blocks: HashMap<usize, ActiveToolBlock>,
    next_choices_tool_index: usize,
    next_block_delta_index: usize,
}

impl NegotiatedTurnsAdapter {
    fn new(mode: TurnsSseMode) -> Self {
        let protocol_variant = match mode {
            TurnsSseMode::BlockDelta | TurnsSseMode::RuntimeEnvelope => ProtocolVariant::BlockDelta,
            TurnsSseMode::ChoicesDelta => ProtocolVariant::ChoicesDelta,
        };

        Self {
            mode,
            mapper: mapper_for_variant(protocol_variant),
            message_id: None,
            active_text_blocks: HashSet::new(),
            pending_tool_blocks: VecDeque::new(),
            active_tool_blocks: HashMap::new(),
            next_choices_tool_index: 0,
            next_block_delta_index: 1_000_000,
        }
    }

    fn map_payload(&mut self, payload: String) -> Vec<Event> {
        let Ok(envelope) = serde_json::from_str::<RuntimeEnvelope>(&payload) else {
            return vec![json_event(
                json!({
                    "type": "error",
                    "error": {
                        "type": "sse_parse_error",
                        "message": "failed to parse runtime envelope"
                    }
                })
                .to_string(),
            )];
        };

        match self.mode {
            TurnsSseMode::RuntimeEnvelope => vec![runtime_event(payload)],
            TurnsSseMode::BlockDelta => self.map_block_delta(envelope),
            TurnsSseMode::ChoicesDelta => self.map_choices_delta(envelope),
        }
    }

    fn map_block_delta(&mut self, envelope: RuntimeEnvelope) -> Vec<Event> {
        match envelope.event {
            RuntimeEvent::TurnStart { .. } => {
                let message_id = format!("{}-turn-{}", envelope.task_id, envelope.turn);
                self.message_id = Some(message_id.clone());
                vec![json_event(
                    json!({
                        "type": "message_start",
                        "message": {
                            "id": message_id,
                            "type": "message",
                            "role": "assistant",
                            "model": "local-api",
                            "content": [],
                            "stop_reason": serde_json::Value::Null,
                            "stop_sequence": serde_json::Value::Null
                        }
                    })
                    .to_string(),
                )]
            }
            RuntimeEvent::TranscriptBlockStart { index, block } => match block {
                StreamBlock::ToolCall { input, .. } => {
                    self.pending_tool_blocks.push_back(PendingToolBlock {
                        block_index: index,
                        fallback_arguments_json: serialize_nonempty_arguments(&input),
                    });
                    Vec::new()
                }
                StreamBlock::FinalText { content } | StreamBlock::Thinking { content, .. } => {
                    self.active_text_blocks.insert(index);
                    vec![json_event(
                        json!({
                            "type": "content_block_start",
                            "index": index,
                            "content_block": ContentBlock::Text {
                                text: content,
                                citations: None,
                            }
                        })
                        .to_string(),
                    )]
                }
                StreamBlock::ToolResult { .. } => Vec::new(),
            },
            RuntimeEvent::TranscriptBlockDelta { index, delta } => {
                if let Some(active) = self.active_tool_blocks.get_mut(&index) {
                    active.saw_delta = true;
                    return vec![event_from_sse_frame(
                        self.mapper.tool_delta_frame(active.wire_index, &delta),
                    )];
                }

                if self.active_text_blocks.contains(&index) {
                    return vec![json_event(
                        json!({
                            "type": "content_block_delta",
                            "index": index,
                            "delta": {
                                "type": "text_delta",
                                "text": delta,
                            }
                        })
                        .to_string(),
                    )];
                }

                Vec::new()
            }
            RuntimeEvent::TranscriptBlockComplete { index } => {
                if let Some(active) = self.active_tool_blocks.remove(&index) {
                    return self.finish_block_delta_tool(active);
                }

                if self.active_text_blocks.remove(&index) {
                    return vec![json_event(
                        json!({
                            "type": "content_block_stop",
                            "index": index,
                        })
                        .to_string(),
                    )];
                }

                Vec::new()
            }
            RuntimeEvent::ToolCall {
                id,
                name,
                arguments,
            } => {
                if let Some(pending) = self.pending_tool_blocks.pop_front() {
                    let wire_index = pending.block_index;
                    self.active_tool_blocks.insert(
                        pending.block_index,
                        ActiveToolBlock {
                            wire_index,
                            fallback_arguments_json: pending
                                .fallback_arguments_json
                                .or_else(|| serialize_nonempty_arguments(&arguments)),
                            saw_delta: false,
                        },
                    );
                    return vec![event_from_sse_frame(
                        self.mapper.tool_start_frame(wire_index, &id, &name),
                    )];
                }

                let wire_index = self.next_block_delta_index;
                self.next_block_delta_index = self.next_block_delta_index.saturating_add(1);
                let mut events = vec![event_from_sse_frame(
                    self.mapper.tool_start_frame(wire_index, &id, &name),
                )];
                if let Some(serialized) = serialize_nonempty_arguments(&arguments) {
                    events.push(event_from_sse_frame(
                        self.mapper.tool_delta_frame(wire_index, &serialized),
                    ));
                }
                events.push(event_from_sse_frame(
                    self.mapper.tool_finish_frame(wire_index),
                ));
                events
            }
            RuntimeEvent::TurnEnd {
                status: _, usage, ..
            } => {
                let mut events = self.close_open_block_delta_tools();
                for index in self.active_text_blocks.drain() {
                    events.push(json_event(
                        json!({
                            "type": "content_block_stop",
                            "index": index,
                        })
                        .to_string(),
                    ));
                }
                events.push(json_event(
                    json!({
                        "type": "message_delta",
                        "delta": {
                            "stop_reason": "end_turn",
                            "stop_sequence": serde_json::Value::Null,
                        },
                        "usage": usage.map(token_usage_to_api_usage),
                    })
                    .to_string(),
                ));
                events.push(json_event(json!({"type": "message_stop"}).to_string()));
                events
            }
            RuntimeEvent::Error { code, message, .. } => vec![json_event(
                json!({
                    "type": "error",
                    "error": {
                        "type": code,
                        "message": message,
                    }
                })
                .to_string(),
            )],
            RuntimeEvent::MaxTurnsReached { max_turns } => vec![json_event(
                json!({
                    "type": "error",
                    "error": {
                        "type": "max_turns_reached",
                        "message": format!("max turns reached: {max_turns}"),
                    }
                })
                .to_string(),
            )],
            RuntimeEvent::TranscriptLine { .. }
            | RuntimeEvent::ToolCallStatusUpdated { .. }
            | RuntimeEvent::TranscriptBlockPhaseUpdated { .. }
            | RuntimeEvent::ToolResult { .. }
            | RuntimeEvent::ApprovalRequest { .. }
            | RuntimeEvent::ApprovalResolved { .. }
            | RuntimeEvent::ValidationResult { .. } => Vec::new(),
        }
    }

    fn map_choices_delta(&mut self, envelope: RuntimeEnvelope) -> Vec<Event> {
        match envelope.event {
            RuntimeEvent::TurnStart { .. } => {
                let message_id = format!("{}-turn-{}", envelope.task_id, envelope.turn);
                self.message_id = Some(message_id.clone());
                vec![json_event(
                    json!({
                        "id": message_id,
                        "object": "chat.completion.chunk",
                        "model": "local-api",
                        "choices": [{
                            "index": 0,
                            "delta": {"role": "assistant"},
                            "finish_reason": serde_json::Value::Null,
                        }]
                    })
                    .to_string(),
                )]
            }
            RuntimeEvent::TranscriptBlockStart { index, block } => {
                if let StreamBlock::ToolCall { input, .. } = block {
                    self.pending_tool_blocks.push_back(PendingToolBlock {
                        block_index: index,
                        fallback_arguments_json: serialize_nonempty_arguments(&input),
                    });
                }
                Vec::new()
            }
            RuntimeEvent::TranscriptBlockDelta { index, delta } => {
                if let Some(active) = self.active_tool_blocks.get_mut(&index) {
                    active.saw_delta = true;
                    return vec![json_event(augment_choices_chunk(
                        self.message_id.as_deref(),
                        event_payload_from_sse_frame(
                            self.mapper.tool_delta_frame(active.wire_index, &delta),
                        ),
                    ))];
                }

                vec![json_event(text_choices_chunk(
                    self.message_id.as_deref(),
                    &delta,
                    None,
                    None,
                ))]
            }
            RuntimeEvent::TranscriptBlockComplete { index } => {
                if let Some(active) = self.active_tool_blocks.remove(&index) {
                    return self.finish_choices_tool(active);
                }
                Vec::new()
            }
            RuntimeEvent::ToolCall {
                id,
                name,
                arguments,
            } => {
                if let Some(pending) = self.pending_tool_blocks.pop_front() {
                    let wire_index = self.next_choices_tool_index;
                    self.next_choices_tool_index = self.next_choices_tool_index.saturating_add(1);
                    self.active_tool_blocks.insert(
                        pending.block_index,
                        ActiveToolBlock {
                            wire_index,
                            fallback_arguments_json: pending
                                .fallback_arguments_json
                                .or_else(|| serialize_nonempty_arguments(&arguments)),
                            saw_delta: false,
                        },
                    );
                    return vec![json_event(augment_choices_chunk(
                        self.message_id.as_deref(),
                        event_payload_from_sse_frame(
                            self.mapper.tool_start_frame(wire_index, &id, &name),
                        ),
                    ))];
                }

                let wire_index = self.next_choices_tool_index;
                self.next_choices_tool_index = self.next_choices_tool_index.saturating_add(1);
                let mut events = vec![json_event(augment_choices_chunk(
                    self.message_id.as_deref(),
                    event_payload_from_sse_frame(
                        self.mapper.tool_start_frame(wire_index, &id, &name),
                    ),
                ))];
                if let Some(serialized) = serialize_nonempty_arguments(&arguments) {
                    events.push(json_event(augment_choices_chunk(
                        self.message_id.as_deref(),
                        event_payload_from_sse_frame(
                            self.mapper.tool_delta_frame(wire_index, &serialized),
                        ),
                    )));
                }
                events.push(json_event(augment_choices_chunk(
                    self.message_id.as_deref(),
                    event_payload_from_sse_frame(self.mapper.tool_finish_frame(wire_index)),
                )));
                events
            }
            RuntimeEvent::TurnEnd { usage, .. } => {
                let mut events = self.close_open_choices_tools();
                events.push(json_event(text_choices_chunk(
                    self.message_id.as_deref(),
                    "",
                    Some("stop"),
                    usage.map(token_usage_to_api_usage),
                )));
                events.push(json_event("[DONE]".to_string()));
                events
            }
            RuntimeEvent::Error { code, message, .. } => vec![json_event(
                json!({
                    "type": "error",
                    "error": {
                        "type": code,
                        "message": message,
                    }
                })
                .to_string(),
            )],
            RuntimeEvent::MaxTurnsReached { max_turns } => vec![json_event(
                json!({
                    "type": "error",
                    "error": {
                        "type": "max_turns_reached",
                        "message": format!("max turns reached: {max_turns}"),
                    }
                })
                .to_string(),
            )],
            RuntimeEvent::TranscriptLine { .. }
            | RuntimeEvent::ToolCallStatusUpdated { .. }
            | RuntimeEvent::TranscriptBlockPhaseUpdated { .. }
            | RuntimeEvent::ToolResult { .. }
            | RuntimeEvent::ApprovalRequest { .. }
            | RuntimeEvent::ApprovalResolved { .. }
            | RuntimeEvent::ValidationResult { .. } => Vec::new(),
        }
    }

    fn finish_block_delta_tool(&self, active: ActiveToolBlock) -> Vec<Event> {
        let mut events = Vec::new();
        if !active.saw_delta
            && let Some(serialized) = active.fallback_arguments_json.as_deref()
        {
            events.push(event_from_sse_frame(
                self.mapper.tool_delta_frame(active.wire_index, serialized),
            ));
        }
        events.push(event_from_sse_frame(
            self.mapper.tool_finish_frame(active.wire_index),
        ));
        events
    }

    fn finish_choices_tool(&self, active: ActiveToolBlock) -> Vec<Event> {
        let mut events = Vec::new();
        if !active.saw_delta
            && let Some(serialized) = active.fallback_arguments_json.as_deref()
        {
            events.push(json_event(augment_choices_chunk(
                self.message_id.as_deref(),
                event_payload_from_sse_frame(
                    self.mapper.tool_delta_frame(active.wire_index, serialized),
                ),
            )));
        }
        events.push(json_event(augment_choices_chunk(
            self.message_id.as_deref(),
            event_payload_from_sse_frame(self.mapper.tool_finish_frame(active.wire_index)),
        )));
        events
    }

    fn close_open_block_delta_tools(&mut self) -> Vec<Event> {
        let active = std::mem::take(&mut self.active_tool_blocks);
        active
            .into_values()
            .flat_map(|tool| self.finish_block_delta_tool(tool))
            .collect()
    }

    fn close_open_choices_tools(&mut self) -> Vec<Event> {
        let active = std::mem::take(&mut self.active_tool_blocks);
        active
            .into_values()
            .flat_map(|tool| self.finish_choices_tool(tool))
            .collect()
    }
}

pub fn negotiate_turns_sse_mode(accept_header: Option<&str>) -> TurnsSseMode {
    let accept = accept_header.unwrap_or_default().to_ascii_lowercase();
    if accept.contains("application/vnd.block-delta+sse") {
        return TurnsSseMode::BlockDelta;
    }
    if accept.contains("application/vnd.choices-delta+sse") {
        return TurnsSseMode::ChoicesDelta;
    }
    TurnsSseMode::RuntimeEnvelope
}

pub fn runtime_sse_response(
    envelope_rx: mpsc::UnboundedReceiver<String>,
    keepalive_interval: Duration,
    mode: TurnsSseMode,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream: Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> = match mode {
        TurnsSseMode::RuntimeEnvelope => UnboundedReceiverStream::new(envelope_rx)
            .map(|payload| Ok::<Event, Infallible>(runtime_event(payload)))
            .boxed(),
        TurnsSseMode::BlockDelta | TurnsSseMode::ChoicesDelta => {
            let mut adapter = NegotiatedTurnsAdapter::new(mode);
            UnboundedReceiverStream::new(envelope_rx)
                .flat_map(move |payload| {
                    stream::iter(
                        adapter
                            .map_payload(payload)
                            .into_iter()
                            .map(Ok::<Event, Infallible>),
                    )
                })
                .boxed()
        }
    };
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(keepalive_interval)
            .text(SSE_KEEPALIVE_TEXT),
    )
}

fn runtime_event(payload: String) -> Event {
    Event::default()
        .event("runtime")
        .id(Utc::now().to_rfc3339())
        .data(payload)
}

fn json_event(payload: String) -> Event {
    Event::default().id(Utc::now().to_rfc3339()).data(payload)
}

fn event_from_sse_frame(frame: SseFrame) -> Event {
    json_event(event_payload_from_sse_frame(frame))
}

fn event_payload_from_sse_frame(frame: SseFrame) -> String {
    frame
        .0
        .strip_prefix("data: ")
        .unwrap_or(frame.0.as_str())
        .trim_end_matches('\n')
        .trim_end_matches('\n')
        .to_string()
}

fn serialize_nonempty_arguments(arguments: &serde_json::Value) -> Option<String> {
    let serialized = serde_json::to_string(arguments).ok()?;
    if serialized == "{}" {
        None
    } else {
        Some(serialized)
    }
}

fn token_usage_to_api_usage(usage: TokenUsageEnvelope) -> serde_json::Value {
    json!({
        "input_tokens": usage.input,
        "output_tokens": usage.output,
        "total_tokens": usage.input.saturating_add(usage.output),
    })
}

fn text_choices_chunk(
    message_id: Option<&str>,
    text: &str,
    finish_reason: Option<&str>,
    usage: Option<serde_json::Value>,
) -> String {
    let mut payload = json!({
        "choices": [{
            "index": 0,
            "delta": if text.is_empty() { json!({}) } else { json!({"content": text}) },
            "finish_reason": finish_reason,
        }]
    });
    if let Some(message_id) = message_id {
        payload["id"] = json!(message_id);
        payload["object"] = json!("chat.completion.chunk");
        payload["model"] = json!("local-api");
    }
    if let Some(usage) = usage {
        payload["usage"] = usage;
    }
    payload.to_string()
}

fn augment_choices_chunk(message_id: Option<&str>, payload: String) -> String {
    let mut value = serde_json::from_str::<serde_json::Value>(&payload)
        .unwrap_or_else(|_| json!({"choices": []}));
    if let Some(message_id) = message_id {
        value["id"] = json!(message_id);
        value["object"] = json!("chat.completion.chunk");
        value["model"] = json!("local-api");
    }
    value.to_string()
}
