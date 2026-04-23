use self::provider::ProviderStreamEvent;

use self::ingress_adapter::IngressProtocolAdapter;

pub(crate) mod chat_compat;
mod ingress_adapter;
pub(crate) mod provider;
mod sse_framing;
mod text_normaliser;

pub(crate) use self::chat_compat::ChatCompatPayload;
pub use self::text_normaliser::{NormalisedChunk, StreamTextNormaliser};

const MAX_SSE_BUFFER_BYTES: usize = 1_048_576;

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
