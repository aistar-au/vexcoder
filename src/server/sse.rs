//! SSE framing for the local API server.
//!
//! The server emits canonical RuntimeEnvelope JSON as ordinary
//! `text/event-stream` data frames. Event IDs remain intentionally omitted
//! pending future resumable replay support.

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::{Stream, StreamExt};
use std::convert::Infallible;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

use super::SSE_KEEPALIVE_TEXT;

pub fn runtime_sse_response(
    envelope_rx: mpsc::UnboundedReceiver<String>,
    keepalive_interval: Duration,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = UnboundedReceiverStream::new(envelope_rx)
        .map(|payload| Ok::<Event, Infallible>(Event::default().data(payload)));

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(keepalive_interval)
            .text(SSE_KEEPALIVE_TEXT),
    )
}
