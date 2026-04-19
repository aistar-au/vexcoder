use super::{IngressPayload, StreamOutputMode, StreamParser};
use crate::runtime::json_handoff::RuntimeEnvelopeNormalizer;
use crate::runtime::{RuntimeEnvelope, RuntimeEvent};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_STREAM_TASK_ID: AtomicU64 = AtomicU64::new(1);

pub(super) struct IngressProtocolAdapter {
    normalizer: RuntimeEnvelopeNormalizer,
}

impl IngressProtocolAdapter {
    fn new() -> Self {
        Self {
            normalizer: RuntimeEnvelopeNormalizer::new(next_stream_task_id()),
        }
    }

    fn finish_stream_turn(&mut self) -> Vec<RuntimeEnvelope> {
        self.normalizer.finish_protocol_ingress_turn()
    }

    fn adapt_payload(&mut self, payload: IngressPayload) -> Vec<RuntimeEnvelope> {
        match payload {
            IngressPayload::Provider(event) => {
                self.normalizer.normalize_provider_stream_event(*event)
            }
            IngressPayload::ChatCompat(payload) => {
                self.normalizer.normalize_chat_compat_payload(payload)
            }
        }
    }

    fn emit_protocol_error(&mut self, code: String, message: String) -> RuntimeEnvelope {
        self.normalizer.emit_protocol_ingress_error(code, message)
    }
}

impl StreamParser {
    pub fn finish(&mut self) -> Vec<RuntimeEnvelope> {
        if self.output_mode != StreamOutputMode::ProtocolAdapter {
            return Vec::new();
        }

        self.protocol_adapter_mut().finish_stream_turn()
    }

    pub(super) fn adapt_protocol_payload(
        &mut self,
        payload: IngressPayload,
    ) -> Vec<RuntimeEnvelope> {
        if self.output_mode == StreamOutputMode::RuntimeEnvelope {
            return vec![self.protocol_stream_error_envelope(
                "mixed_sse_protocol",
                "received provider or chat-compatible stream data after RuntimeEnvelope passthrough began"
                    .to_string(),
            )];
        }

        self.output_mode = StreamOutputMode::ProtocolAdapter;
        self.protocol_adapter_mut().adapt_payload(payload)
    }

    pub(super) fn protocol_stream_error_envelope(
        &mut self,
        code: &str,
        message: String,
    ) -> RuntimeEnvelope {
        if self.output_mode == StreamOutputMode::RuntimeEnvelope {
            let mut normalizer = RuntimeEnvelopeNormalizer::new(next_stream_task_id());
            let _ = normalizer.start_turn(1, None);
            return normalizer.emit_event(RuntimeEvent::Error {
                code: code.to_string(),
                message,
                recoverable: true,
            });
        }

        self.output_mode = StreamOutputMode::ProtocolAdapter;
        self.protocol_adapter_mut()
            .emit_protocol_error(code.to_string(), message)
    }

    pub(super) fn enter_runtime_envelope_mode(
        &mut self,
        envelope: RuntimeEnvelope,
    ) -> Vec<RuntimeEnvelope> {
        if self.output_mode == StreamOutputMode::ProtocolAdapter {
            return vec![self.protocol_stream_error_envelope(
                "mixed_sse_protocol",
                "received RuntimeEnvelope passthrough after provider or chat-compatible adaptation began"
                    .to_string(),
            )];
        }

        self.output_mode = StreamOutputMode::RuntimeEnvelope;
        vec![envelope]
    }

    fn protocol_adapter_mut(&mut self) -> &mut IngressProtocolAdapter {
        self.protocol_adapter
            .get_or_insert_with(IngressProtocolAdapter::new)
    }
}

fn next_stream_task_id() -> String {
    let id = NEXT_STREAM_TASK_ID.fetch_add(1, Ordering::Relaxed);
    format!("api_stream_{id}")
}
