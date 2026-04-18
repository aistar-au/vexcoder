use crate::runtime::json_handoff::RuntimeEnvelopeNormalizer;
use crate::usage::TurnTokens;
use std::collections::BTreeSet;

mod chat_compat;
mod framing;
mod provider;
mod text_normaliser;

pub use self::text_normaliser::{NormalisedChunk, StreamTextNormaliser};

/// Maximum number of bytes the SSE intra-frame accumulation buffer may hold.
/// Chunks are drained as soon as a frame delimiter (`\n\n` or `\r\n\r\n`) is
/// found, so this bound is only reached when the upstream stream emits no
/// delimiters. A 1 MiB ceiling matches the editor cap and is far above any
/// real SSE frame. ADR-021 Item 26.
const MAX_SSE_BUFFER_BYTES: usize = 1_048_576;

/// Hard ceiling on the tool-call index accepted from a chat-compat stream.
/// Indices beyond this cap are clamped to prevent unbounded Vec allocation
/// from untrusted server data.
const MAX_TOOL_CALL_INDEX: usize = 1_024;

const THINKING_DATA_TAG: &str = "thinking_data";

fn legacy_external_thinking_tag() -> &'static str {
    concat!("re", "dacted_thinking")
}

fn transitional_internal_thinking_tag() -> &'static str {
    concat!("su", "ppressed_thinking")
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum StreamProtocolMode {
    #[default]
    Undecided,
    RuntimeEnvelope,
    ProviderNormalized,
}

#[derive(Default, Clone)]
struct ChatCompatToolState {
    id: String,
    name: String,
    pending_arguments: String,
    started: bool,
    stopped: bool,
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

impl StreamParser {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests;
