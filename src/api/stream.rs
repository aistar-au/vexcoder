use self::provider::ProviderStreamEvent;

use self::ingress_adapter::IngressProtocolAdapter;

pub(crate) mod chat_compat;
mod ingress_adapter;
pub(crate) mod provider;
mod sse_framing;
mod text_normaliser;

pub(crate) use self::chat_compat::ChatCompatPayload;
pub use self::text_normaliser::{NormalisedChunk, StreamTextNormaliser};

/// Maximum number of bytes the SSE intra-frame accumulation buffer may hold.
/// Chunks are drained as soon as a frame delimiter (`\n\n` or `\r\n\r\n`) is
/// found, so this bound is only reached when the upstream stream emits no
/// delimiters. A 1 MiB ceiling matches the editor cap and is far above any
/// real SSE frame. ADR-021 Item 26.
const MAX_SSE_BUFFER_BYTES: usize = 1_048_576;

/// Strict ceiling on the tool-call index accepted from a chat-compat stream.
/// Indices beyond this cap are clamped to prevent unbounded Vec allocation
/// from untrusted server data.
pub(crate) const MAX_TOOL_CALL_INDEX: usize = 1_024;

pub(crate) enum IngressPayload {
    Provider(Box<ProviderStreamEvent>),
    ChatCompat(ChatCompatPayload),
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum StreamOutputMode {
    #[default]
    Undecided,
    RuntimeEnvelope,
    ProtocolAdapter,
}

#[derive(Default)]
pub struct StreamParser {
    buffer: Vec<u8>,
    bom_checked: bool,
    overflowed: bool,
    // WHATWG HTML §9.2.6 — last received event ID and reconnection delay.
    // Updated as id: and retry: fields are parsed; exposed for callers that
    // implement reconnection with Last-Event-ID semantics.
    last_event_id: Option<String>,
    reconnect_delay_ms: Option<u64>,
    output_mode: StreamOutputMode,
    protocol_adapter: Option<IngressProtocolAdapter>,
}

impl StreamParser {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests;
