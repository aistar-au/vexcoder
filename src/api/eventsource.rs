//! Reqwest-backed EventSource bridge.
//!
//! The upstream streaming endpoints require `POST` request bodies, so this
//! module intentionally uses EventSource framing without the browser
//! `GET`-only interface contract. Reconnect is disabled because retrying a
//! non-idempotent streamed generation request would duplicate work and billing.

use crate::api::client::{map_api_request_error, map_api_status_error};
use crate::api::stream::StreamParser;
use crate::runtime::RuntimeEnvelope;
use crate::runtime::backend::EventStream;
use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use eventsource_client::{Client as _, ClientBuilder, ReconnectOptions, SSE};
use futures::{StreamExt, stream};
use launchdarkly_sdk_transport::{
    ByteStream as TransportByteStream, HttpTransport, ResponseFuture, TransportError,
};
use std::collections::VecDeque;

pub(crate) async fn create_event_stream(
    http: reqwest::Client,
    request_url: &str,
    payload: &serde_json::Value,
    headers: &reqwest::header::HeaderMap,
) -> Result<EventStream> {
    let mut builder = ClientBuilder::for_url(request_url)
        .map_err(|error| {
            anyhow!(
                "failed to build SSE client for '{}': {}",
                request_url,
                error
            )
        })?
        // Intentional deviation from the browser EventSource interface: the
        // upstream APIs require POST bodies for streamed generation.
        .method("POST".to_string())
        .body(payload.to_string())
        // Reconnect stays disabled for POST streaming requests. Replaying a
        // partial generation would not be idempotent and could duplicate work.
        .reconnect(ReconnectOptions::reconnect(false).build());

    for (name, value) in headers {
        let value = value
            .to_str()
            .with_context(|| format!("header '{}' is not valid UTF-8", name.as_str()))?;
        builder = builder.header(name.as_str(), value).map_err(|error| {
            anyhow!(
                "failed to configure SSE header '{}' for '{}': {}",
                name.as_str(),
                request_url,
                error
            )
        })?;
    }

    let client = builder.build_with_transport(ReqwestEventSourceTransport { client: http });
    let mut state = EventStreamState {
        upstream: client.stream(),
        parser: StreamParser::new(),
        pending: VecDeque::new(),
        request_url: request_url.to_string(),
    };

    let first_event = next_stream_event(&mut state).await?;
    let tail = stream::try_unfold(state, |mut state| async move {
        match next_stream_event(&mut state).await {
            Ok(Some(event)) => Ok(Some((event, state))),
            Ok(None) => Ok(None),
            Err(error) => Err(error),
        }
    });

    if let Some(first_event) = first_event {
        Ok(Box::pin(stream::iter(vec![Ok(first_event)]).chain(tail)))
    } else {
        Ok(Box::pin(tail))
    }
}

struct EventStreamState {
    upstream: eventsource_client::BoxStream<eventsource_client::Result<SSE>>,
    parser: StreamParser,
    pending: VecDeque<RuntimeEnvelope>,
    request_url: String,
}

fn finish_pending_events(state: &mut EventStreamState) -> Option<RuntimeEnvelope> {
    state.pending.extend(state.parser.finish());
    state.pending.pop_front()
}

async fn next_stream_event(state: &mut EventStreamState) -> Result<Option<RuntimeEnvelope>> {
    loop {
        if let Some(event) = state.pending.pop_front() {
            return Ok(Some(event));
        }

        let Some(item) = state.upstream.next().await else {
            if let Some(event) = finish_pending_events(state) {
                return Ok(Some(event));
            }
            return Ok(None);
        };

        match item {
            Ok(SSE::Connected(_)) | Ok(SSE::Comment(_)) => continue,
            Ok(SSE::Event(event)) => {
                state.pending.extend(
                    state
                        .parser
                        .process_sse_event(event.event_type.as_str(), event.data.as_str())?,
                );
            }
            Err(eventsource_client::Error::Eof) => {
                if let Some(event) = finish_pending_events(state) {
                    return Ok(Some(event));
                }
                return Ok(None);
            }
            Err(eventsource_client::Error::UnexpectedResponse(response, body)) => {
                let retry_after = response
                    .get_header_value("retry-after")
                    .ok()
                    .flatten()
                    .map(str::to_owned);
                let status = reqwest::StatusCode::from_u16(response.status())
                    .unwrap_or(reqwest::StatusCode::INTERNAL_SERVER_ERROR);
                let body = read_error_body(body.into_stream()).await;
                return Err(map_api_status_error(
                    status,
                    &body,
                    &state.request_url,
                    retry_after.as_deref(),
                ));
            }
            Err(eventsource_client::Error::Transport(error)) => {
                return Err(anyhow!(error.to_string()));
            }
            Err(eventsource_client::Error::TimedOut) => {
                return Err(anyhow!("API request to '{}' timed out", state.request_url));
            }
            Err(error) => {
                return Err(anyhow!(
                    "SSE stream from '{}' failed: {}",
                    state.request_url,
                    error
                ));
            }
        }
    }
}

async fn read_error_body(mut stream: TransportByteStream) -> String {
    const MAX_ERROR_BODY_BYTES: usize = 16 * 1024;

    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) => {
                let remaining = MAX_ERROR_BODY_BYTES.saturating_sub(body.len());
                if remaining == 0 {
                    break;
                }
                let take = remaining.min(chunk.len());
                body.extend_from_slice(&chunk[..take]);
            }
            Err(error) => {
                if body.is_empty() {
                    return format!("<failed to read error body: {error}>");
                }
                break;
            }
        }
    }

    String::from_utf8_lossy(&body).into_owned()
}

#[derive(Clone)]
struct ReqwestEventSourceTransport {
    client: reqwest::Client,
}

impl HttpTransport for ReqwestEventSourceTransport {
    fn request(&self, request: http::Request<Option<Bytes>>) -> ResponseFuture {
        let client = self.client.clone();

        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let request_url = parts.uri.to_string();
            let revised_request_url = crate::runtime::rewrite_url_for_logs(&request_url);

            tracing::debug!(
                target: "vex::http",
                method = %parts.method,
                url = %revised_request_url,
                "sending streaming request"
            );

            let mut reqwest_request = client.request(parts.method, request_url.clone());
            for (name, value) in &parts.headers {
                reqwest_request = reqwest_request.header(name, value);
            }
            if let Some(body) = body {
                reqwest_request = reqwest_request.body(body);
            }

            let response = reqwest_request.send().await.map_err(|error| {
                TransportError::new(std::io::Error::other(
                    map_api_request_error(error, &request_url).to_string(),
                ))
            })?;

            let status = response.status();
            let headers = response.headers().clone();
            let request_url_for_stream = request_url.clone();
            let body: TransportByteStream = Box::pin(response.bytes_stream().map(move |item| {
                item.map_err(|error| {
                    TransportError::new(std::io::Error::other(
                        map_api_request_error(error, &request_url_for_stream).to_string(),
                    ))
                })
            }));

            let mut response_builder = http::Response::builder().status(status);
            for (name, value) in &headers {
                response_builder = response_builder.header(name, value);
            }

            response_builder.body(body).map_err(TransportError::new)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::RuntimeEvent;
    use futures::stream;

    #[tokio::test]
    async fn eof_still_flushes_provider_normalized_turn_end() {
        let mut parser = StreamParser::new();
        let _ = parser
            .process(
                b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":15}}\n\n",
            )
            .unwrap();

        let mut state = EventStreamState {
            upstream: Box::pin(stream::iter(vec![Err(eventsource_client::Error::Eof)])),
            parser,
            pending: VecDeque::new(),
            request_url: "https://example.test/sse".to_string(),
        };

        let event = next_stream_event(&mut state).await.unwrap().unwrap();

        assert!(matches!(event.event, RuntimeEvent::TurnEnd { .. }));
        assert!(next_stream_event(&mut state).await.unwrap().is_none());
    }
}
