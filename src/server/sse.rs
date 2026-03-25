use axum::response::sse::{Event, KeepAlive, Sse};
use std::convert::Infallible;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;

use super::SSE_KEEPALIVE_TEXT;

pub fn runtime_sse_response(
    envelope_rx: mpsc::UnboundedReceiver<String>,
    keepalive_interval: Duration,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = UnboundedReceiverStream::new(envelope_rx)
        .map(|payload| Ok::<Event, Infallible>(Event::default().event("runtime").data(payload)));
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(keepalive_interval)
            .text(SSE_KEEPALIVE_TEXT),
    )
}
