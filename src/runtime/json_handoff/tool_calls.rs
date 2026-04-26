use super::derived::empty_json_object;
use super::{
    RuntimeEnvelope, RuntimeEnvelopeNormalizer, RuntimeSignal, ToolCallId, timestamp_string,
};
use crate::pulse_evidence::note_changed_files_from_tool_call;
use crate::runtime::delta_accumulator::AccumulationError;
use crate::state::ToolStatus;
use chrono::{DateTime, Timelike, Utc};
use std::sync::atomic::Ordering;

pub(super) fn serialize_tool_arguments(arguments: &serde_json::Value) -> String {
    match arguments {
        serde_json::Value::Object(map) if map.is_empty() => String::new(),
        _ => serde_json::to_string(arguments).unwrap_or_default(),
    }
}

#[derive(Debug, Clone)]
pub(super) struct PendingToolCall {
    pub(super) runtime_id: ToolCallId,
    pub(super) name: String,
    pub(super) arguments: serde_json::Value,
    pub(super) raw_arguments: String,
    pub(super) start_frame_id: String,
    pub(super) started_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub(super) struct PendingToolCallContext {
    pub(super) name: String,
    pub(super) start_frame_id: String,
}

impl RuntimeEnvelopeNormalizer {
    pub(super) fn normalize_grammar_tool_call(
        &mut self,
        tool_call_value: &serde_json::Value,
    ) -> Option<RuntimeEnvelope> {
        let name = tool_call_value.get("name")?.as_str()?.to_string();
        let arguments = tool_call_value
            .get("arguments")
            .cloned()
            .unwrap_or_else(empty_json_object);

        let runtime_id = self.generate_tool_call_id();
        let started_at = Utc::now();
        let envelope = self.next_envelope(RuntimeSignal::ToolCallStarted {
            tool_call_id: runtime_id.clone(),
            tool_name: name.clone(),
            arguments: arguments.clone(),
            status: ToolStatus::Pending,
            started_at: timestamp_string(started_at),
        });
        self.pending_tool_calls.insert(
            runtime_id.clone(),
            PendingToolCall {
                runtime_id: runtime_id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
                raw_arguments: serialize_tool_arguments(&arguments),
                start_frame_id: envelope.frame_id.clone(),
                started_at,
            },
        );
        self.pending_tool_call_contexts.insert(
            runtime_id.clone(),
            PendingToolCallContext {
                name: name.clone(),
                start_frame_id: envelope.frame_id.clone(),
            },
        );
        self.start_tool_accumulator(&runtime_id, &name);
        self.seed_tool_accumulator(&runtime_id, &arguments);

        Some(envelope)
    }

    pub(super) fn record_tool_call(
        &mut self,
        source_id: String,
        name: String,
        arguments: serde_json::Value,
    ) -> RuntimeEnvelope {
        let runtime_id = self.generate_tool_call_id();
        let started_at = Utc::now();
        let envelope = self.next_envelope(RuntimeSignal::ToolCallStarted {
            tool_call_id: runtime_id.clone(),
            tool_name: name.clone(),
            arguments: arguments.clone(),
            status: ToolStatus::Pending,
            started_at: timestamp_string(started_at),
        });
        self.pending_tool_calls.insert(
            source_id,
            PendingToolCall {
                runtime_id: runtime_id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
                raw_arguments: serialize_tool_arguments(&arguments),
                start_frame_id: envelope.frame_id.clone(),
                started_at,
            },
        );
        self.pending_tool_call_contexts.insert(
            runtime_id.clone(),
            PendingToolCallContext {
                name: name.clone(),
                start_frame_id: envelope.frame_id.clone(),
            },
        );
        self.start_tool_accumulator(&runtime_id, &name);
        self.seed_tool_accumulator(&runtime_id, &arguments);

        envelope
    }

    pub(super) fn record_tool_result(
        &mut self,
        source_id: &str,
        output: String,
        is_error: bool,
    ) -> RuntimeEnvelope {
        let completed_at = Utc::now();
        let completed_at_value = timestamp_string(completed_at);
        if let Some(pending) = self.pending_tool_calls.shift_remove(source_id) {
            self.pending_tool_call_contexts.remove(&pending.runtime_id);
            self.finish_tool_accumulator(&pending.runtime_id);
            if !is_error {
                note_changed_files_from_tool_call(
                    &mut self.turn_changed_files,
                    &pending.name,
                    &pending.arguments,
                );
            }
            let duration_ms = completed_at
                .signed_duration_since(pending.started_at)
                .num_milliseconds()
                .max(0) as u64;
            let signal = if is_error {
                RuntimeSignal::ToolCallFailed {
                    tool_call_id: pending.runtime_id,
                    tool_name: Some(pending.name),
                    status: ToolStatus::Error,
                    started_at: Some(timestamp_string(pending.started_at)),
                    completed_at: completed_at_value,
                    duration_ms: Some(duration_ms),
                    output,
                }
            } else {
                RuntimeSignal::ToolCallCompleted {
                    tool_call_id: pending.runtime_id,
                    tool_name: Some(pending.name),
                    status: ToolStatus::Complete,
                    started_at: Some(timestamp_string(pending.started_at)),
                    completed_at: completed_at_value,
                    duration_ms: Some(duration_ms),
                    output,
                }
            };
            return self.next_envelope_with_context(signal, None, Some(pending.start_frame_id));
        }

        self.next_envelope(if is_error {
            RuntimeSignal::ToolCallFailed {
                tool_call_id: source_id.to_string(),
                tool_name: None,
                status: ToolStatus::Error,
                started_at: None,
                completed_at: completed_at_value,
                duration_ms: None,
                output,
            }
        } else {
            RuntimeSignal::ToolCallCompleted {
                tool_call_id: source_id.to_string(),
                tool_name: None,
                status: ToolStatus::Complete,
                started_at: None,
                completed_at: completed_at_value,
                duration_ms: None,
                output,
            }
        })
    }

    pub(super) fn record_tool_call_arguments_delta(
        &mut self,
        tool_call_id: &ToolCallId,
        delta: &str,
    ) -> RuntimeEnvelope {
        let (tool_name, parent_frame_id) = self
            .pending_tool_call_contexts
            .get(tool_call_id)
            .map(|pending| {
                (
                    Some(pending.name.clone()),
                    Some(pending.start_frame_id.clone()),
                )
            })
            .unwrap_or((None, None));
        let invalid_json = matches!(
            self.accumulate_tool_delta(tool_call_id, delta),
            Some(AccumulationError::MalformedPartial)
        );
        let arguments = self.append_pending_tool_delta(tool_call_id, delta);

        self.next_envelope_with_context(
            RuntimeSignal::ToolCallArgumentsDelta {
                tool_call_id: tool_call_id.clone(),
                tool_name,
                delta: delta.to_string(),
                status: ToolStatus::Pending,
                arguments,
                invalid_json,
            },
            None,
            parent_frame_id,
        )
    }

    pub(super) fn resolve_changed_files(&self, changed_files: Vec<String>) -> Vec<String> {
        if changed_files.is_empty() {
            self.turn_changed_files.iter().cloned().collect()
        } else {
            changed_files
        }
    }

    fn start_tool_accumulator(&self, tool_call_id: &ToolCallId, name: &str) {
        let Some(delta_accumulator) = &self.delta_accumulator else {
            return;
        };

        if let Err(error) = delta_accumulator.start_tool(
            tool_call_id.clone(),
            self.task_id.clone(),
            name.to_string(),
        ) {
            tracing::debug!(%error, tool_call_id = %tool_call_id, "failed to start tool delta accumulation");
        }
    }

    fn seed_tool_accumulator(&self, tool_call_id: &ToolCallId, arguments: &serde_json::Value) {
        let Some(delta_accumulator) = &self.delta_accumulator else {
            return;
        };

        let Ok(serialized) = serde_json::to_string(arguments) else {
            return;
        };
        if serialized == "{}" {
            return;
        }

        if let Err(error) = delta_accumulator.accumulate(tool_call_id, &serialized) {
            tracing::debug!(%error, tool_call_id = %tool_call_id, "failed to seed tool delta accumulation");
        }
    }

    fn accumulate_tool_delta(
        &self,
        tool_call_id: &ToolCallId,
        delta: &str,
    ) -> Option<AccumulationError> {
        let Some(delta_accumulator) = &self.delta_accumulator else {
            return None;
        };

        if let Err(error) = delta_accumulator.accumulate(tool_call_id, delta) {
            tracing::debug!(%error, tool_call_id = %tool_call_id, "failed to accumulate tool delta");
            return Some(error);
        }

        None
    }

    pub(super) fn finish_tool_accumulator(&self, tool_call_id: &ToolCallId) {
        let Some(delta_accumulator) = &self.delta_accumulator else {
            return;
        };

        delta_accumulator.finish(tool_call_id);
    }

    fn append_pending_tool_delta(
        &mut self,
        tool_call_id: &ToolCallId,
        delta: &str,
    ) -> Option<serde_json::Value> {
        let pending = self
            .pending_tool_calls
            .values_mut()
            .find(|pending| pending.runtime_id == *tool_call_id)?;

        pending.raw_arguments.push_str(delta);
        let arguments = serde_json::from_str::<serde_json::Value>(&pending.raw_arguments)
            .or_else(|_| serde_json::from_str(pending.raw_arguments.trim()))
            .ok()?;
        pending.arguments = arguments.clone();
        Some(arguments)
    }

    pub(super) fn finish_pending_tool_accumulations(&self) {
        for pending in self.pending_tool_calls.values() {
            self.finish_tool_accumulator(&pending.runtime_id);
        }
    }

    fn generate_tool_call_id(&mut self) -> ToolCallId {
        let now = Utc::now();
        let entropy = ((now.nanosecond() as u128
            ^ self.tool_id_counter.load(Ordering::SeqCst) as u128)
            & 0xffff) as u16;
        super::generate_tool_call_id(&self.tool_id_counter, entropy)
    }
}
