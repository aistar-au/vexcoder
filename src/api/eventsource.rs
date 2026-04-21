//! Reqwest-backed EventSource bridge.
//!
//! The upstream streaming endpoints require `POST` request bodies, so this
//! module intentionally uses EventSource framing without the browser
//! `GET`-only interface contract. Reconnect is disabled because retrying a
//! non-idempotent streamed generation request would duplicate work and billing.

mod non_stream;

use crate::api::client::{map_api_request_error, map_api_status_error};
use crate::api::stream::StreamParser;
use crate::runtime::backend::EventStream;
use crate::runtime::{ModelProtocol, RuntimeEnvelope, RuntimeEvent, TokenUsageEnvelope};
use anyhow::Result;
use backoff::ExponentialBackoffBuilder;
use backoff::backoff::Backoff;
use bytes::Bytes;
use futures::{StreamExt, stream};
use std::collections::VecDeque;
use std::future::Future;
use std::time::{Duration, Instant};

#[cfg(test)]
const LOCAL_STREAM_START_TIMEOUT: Duration = Duration::from_millis(50);
#[cfg(not(test))]
const LOCAL_STREAM_START_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const LOCAL_NON_STREAM_FALLBACK_TIMEOUT: Duration = Duration::from_millis(100);
#[cfg(not(test))]
const LOCAL_NON_STREAM_FALLBACK_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(test)]
const LOCAL_CONNECT_RETRY_MAX_ELAPSED: Duration = Duration::from_millis(250);
#[cfg(not(test))]
const LOCAL_CONNECT_RETRY_MAX_ELAPSED: Duration = Duration::from_secs(2);
const LOCAL_CONNECT_RETRY_INITIAL_INTERVAL: Duration = Duration::from_millis(100);
const LOCAL_CONNECT_RETRY_MAX_INTERVAL: Duration = Duration::from_millis(400);

type UpstreamByteStream =
    futures::stream::BoxStream<'static, std::result::Result<Bytes, anyhow::Error>>;

pub(crate) async fn create_event_stream(
    http: reqwest::Client,
    request_url: &str,
    payload: &serde_json::Value,
    serialized_payload: String,
    headers: &reqwest::header::HeaderMap,
    protocol: ModelProtocol,
    request_id: &str,
) -> Result<EventStream> {
    let request_start = Instant::now();
    let is_local_endpoint = crate::util::is_local_endpoint_url(request_url);
    let (state, first_event) = if is_local_endpoint {
        match tokio::time::timeout(LOCAL_STREAM_START_TIMEOUT, async {
            let response = send_streaming_request(
                http.clone(),
                request_url,
                serialized_payload.clone(),
                headers,
                request_id,
            )
            .await?;
            let mut state = build_event_stream_state(
                response,
                request_url,
                request_id,
                protocol,
                request_start,
            );
            let first_event = next_stream_event(&mut state).await?;
            Ok::<_, anyhow::Error>((state, first_event))
        })
        .await
        {
            Ok(result) => result?,
            Err(_) => {
                tracing::warn!(
                    target: "vex::http",
                    request_id = %request_id,
                    url = %crate::runtime::rewrite_url_for_logs(request_url),
                    timeout_ms = LOCAL_STREAM_START_TIMEOUT.as_millis(),
                    elapsed_ms = request_start.elapsed().as_millis() as u64,
                    "local streaming request emitted no initial SSE event; retrying as non-streaming JSON"
                );
                return non_stream::create_non_stream_fallback_stream(
                    http,
                    request_url,
                    payload,
                    headers,
                    protocol,
                    request_id,
                    "no_initial_sse_event",
                )
                .await;
            }
        }
    } else {
        let response = send_streaming_request(
            http.clone(),
            request_url,
            serialized_payload,
            headers,
            request_id,
        )
        .await?;
        let mut state =
            build_event_stream_state(response, request_url, request_id, protocol, request_start);
        let first_event = next_stream_event(&mut state).await?;
        (state, first_event)
    };

    if is_local_endpoint && first_event.is_none() {
        tracing::warn!(
            target: "vex::http",
            request_id = %request_id,
            url = %crate::runtime::rewrite_url_for_logs(request_url),
            elapsed_ms = request_start.elapsed().as_millis() as u64,
            "local streaming request closed before the first SSE event; retrying as non-streaming JSON"
        );
        return non_stream::create_non_stream_fallback_stream(
            http,
            request_url,
            payload,
            headers,
            protocol,
            request_id,
            "no_initial_sse_event",
        )
        .await;
    }

    let tail = stream::try_unfold(state, |mut state| async move {
        match next_stream_event(&mut state).await {
            Ok(Some(event)) => Ok(Some((event, state))),
            Ok(None) => Ok(None),
            Err(error) => Err(error),
        }
    });

    if let Some(first_event) = first_event {
        tracing::debug!(
            target: "vex::http",
            request_id = %request_id,
            url = %crate::runtime::rewrite_url_for_logs(request_url),
            protocol = ?protocol,
            elapsed_ms = request_start.elapsed().as_millis() as u64,
            "streaming response established"
        );
        Ok(Box::pin(stream::iter(vec![Ok(first_event)]).chain(tail)))
    } else {
        Ok(Box::pin(tail))
    }
}

struct EventStreamState {
    upstream: UpstreamByteStream,
    parser: StreamParser,
    pending: VecDeque<RuntimeEnvelope>,
    terminal_outcome: Option<&'static str>,
    request_url: String,
    request_id: String,
    protocol_name: &'static str,
    started_at: Instant,
    envelope_count: u64,
    tool_call_starts: u64,
    tool_call_completions: u64,
    tool_call_failures: u64,
    last_usage: Option<TokenUsageEnvelope>,
    final_status: Option<String>,
    summary_emitted: bool,
}

fn build_event_stream_state(
    response: reqwest::Response,
    request_url: &str,
    request_id: &str,
    protocol: ModelProtocol,
    request_start: Instant,
) -> EventStreamState {
    EventStreamState {
        upstream: response_to_upstream_bytes(response, request_url),
        parser: StreamParser::new(),
        pending: VecDeque::new(),
        terminal_outcome: None,
        request_url: request_url.to_string(),
        request_id: request_id.to_string(),
        protocol_name: protocol_name(protocol),
        started_at: request_start,
        envelope_count: 0,
        tool_call_starts: 0,
        tool_call_completions: 0,
        tool_call_failures: 0,
        last_usage: None,
        final_status: None,
        summary_emitted: false,
    }
}
fn finish_pending_events(state: &mut EventStreamState) -> Option<RuntimeEnvelope> {
    state.pending.extend(state.parser.finish());
    state.pending.pop_front()
}

fn finish_stream(state: &mut EventStreamState, outcome: &'static str) -> Option<RuntimeEnvelope> {
    state.terminal_outcome = Some(outcome);
    if let Some(event) = finish_pending_events(state) {
        observe_envelope(state, &event);
        return Some(event);
    }

    emit_stream_summary(state, outcome);
    None
}

fn observe_envelope(state: &mut EventStreamState, envelope: &RuntimeEnvelope) {
    state.envelope_count += 1;
    match &envelope.event {
        RuntimeEvent::ToolCallStarted { .. } => state.tool_call_starts += 1,
        RuntimeEvent::ToolCallCompleted { .. } => state.tool_call_completions += 1,
        RuntimeEvent::ToolCallFailed { .. } => state.tool_call_failures += 1,
        RuntimeEvent::UsageUpdated { usage } => state.last_usage = Some(usage.clone()),
        RuntimeEvent::TurnEnd { status, usage, .. } => {
            state.final_status = Some(status.clone());
            if let Some(usage) = usage {
                state.last_usage = Some(usage.clone());
            }
        }
        RuntimeEvent::Error { code, .. } => {
            state.final_status = Some(format!("error:{code}"));
        }
        _ => {}
    }
}

fn emit_stream_summary(state: &mut EventStreamState, outcome: &'static str) {
    if state.summary_emitted {
        return;
    }
    state.summary_emitted = true;

    let usage = state.last_usage.as_ref();
    tracing::info!(
        target: "vex::http",
        request_id = %state.request_id,
        url = %crate::runtime::rewrite_url_for_logs(&state.request_url),
        protocol = state.protocol_name,
        outcome,
        elapsed_ms = state.started_at.elapsed().as_millis() as u64,
        envelope_count = state.envelope_count,
        tool_call_starts = state.tool_call_starts,
        tool_call_completions = state.tool_call_completions,
        tool_call_failures = state.tool_call_failures,
        final_status = state.final_status.as_deref().unwrap_or("unknown"),
        usage_input_tokens = usage.map(|usage| usage.input).unwrap_or_default(),
        usage_output_tokens = usage.map(|usage| usage.output).unwrap_or_default(),
        usage_estimated = usage.map(|usage| usage.estimated).unwrap_or(false),
        "stream lifecycle summary"
    );
}

fn protocol_name(protocol: ModelProtocol) -> &'static str {
    match protocol {
        ModelProtocol::MessagesV1 => "messages-v1",
        ModelProtocol::ChatCompat => "chat-compat",
    }
}

async fn next_stream_event(state: &mut EventStreamState) -> Result<Option<RuntimeEnvelope>> {
    loop {
        if let Some(event) = state.pending.pop_front() {
            observe_envelope(state, &event);
            return Ok(Some(event));
        }

        if let Some(outcome) = state.terminal_outcome {
            emit_stream_summary(state, outcome);
            return Ok(None);
        }

        let Some(item) = state.upstream.next().await else {
            return Ok(finish_stream(state, "stream_closed"));
        };

        match item {
            Ok(chunk) => {
                if chunk.is_empty() {
                    continue;
                }
                let parsed = state.parser.process(&chunk)?;
                state.pending.extend(parsed);
                if state.parser.protocol_stream_terminated() {
                    state.pending.extend(state.parser.finish());
                    state.terminal_outcome = Some("protocol_done");
                }
            }
            Err(error) => {
                tracing::warn!(
                    target: "vex::http",
                    request_id = %state.request_id,
                    url = %crate::runtime::rewrite_url_for_logs(&state.request_url),
                    error = %error,
                    "streaming transport error"
                );
                emit_stream_summary(state, "transport_error");
                return Err(error);
            }
        }
    }
}

async fn read_error_body<S, E>(mut stream: S) -> String
where
    S: futures::Stream<Item = std::result::Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
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

async fn send_streaming_request(
    http: reqwest::Client,
    request_url: &str,
    serialized_payload: String,
    headers: &reqwest::header::HeaderMap,
    request_id: &str,
) -> Result<reqwest::Response> {
    let revised_request_url = crate::runtime::rewrite_url_for_logs(request_url);
    tracing::debug!(
        target: "vex::http",
        request_id = %request_id,
        method = "POST",
        url = %revised_request_url,
        "sending streaming request"
    );

    let response = retry_local_connect_errors(request_url, request_id, "streaming_request", || {
        let http = http.clone();
        let request_url = request_url.to_string();
        let headers = headers.clone();
        let serialized_payload = serialized_payload.clone();
        async move {
            let mut request = http.post(request_url).body(serialized_payload);
            for (name, value) in &headers {
                request = request.header(name, value);
            }
            request.send().await
        }
    })
    .await
    .map_err(|error| map_api_request_error(error, request_url))?;

    tracing::debug!(
        target: "vex::http",
        request_id = %request_id,
        url = %revised_request_url,
        status = response.status().as_u16(),
        "streaming request connected"
    );

    if !response.status().is_success() {
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let status = response.status();
        let body = read_error_body(response.bytes_stream()).await;
        tracing::warn!(
            target: "vex::http",
            request_id = %request_id,
            url = %revised_request_url,
            status = status.as_u16(),
            body_bytes = body.len(),
            "streaming endpoint returned an unexpected response"
        );
        return Err(map_api_status_error(
            status,
            &body,
            request_url,
            retry_after.as_deref(),
        ));
    }

    Ok(response)
}

fn response_to_upstream_bytes(
    response: reqwest::Response,
    request_url: &str,
) -> UpstreamByteStream {
    let request_url = request_url.to_string();
    Box::pin(
        response
            .bytes_stream()
            .map(move |item| item.map_err(|error| map_api_request_error(error, &request_url))),
    )
}

fn local_connect_retry_backoff() -> backoff::ExponentialBackoff {
    let mut builder = ExponentialBackoffBuilder::new();
    builder
        .with_initial_interval(LOCAL_CONNECT_RETRY_INITIAL_INTERVAL)
        .with_randomization_factor(0.0)
        .with_multiplier(2.0)
        .with_max_interval(LOCAL_CONNECT_RETRY_MAX_INTERVAL)
        .with_max_elapsed_time(Some(LOCAL_CONNECT_RETRY_MAX_ELAPSED));
    builder.build()
}

fn should_retry_local_startup_error(error: &reqwest::Error) -> bool {
    error.status().is_none() && (error.is_connect() || error.is_timeout())
}

async fn retry_local_connect_errors<T, Op, Fut>(
    request_url: &str,
    request_id: &str,
    operation_name: &'static str,
    mut operation: Op,
) -> std::result::Result<T, reqwest::Error>
where
    Op: FnMut() -> Fut,
    Fut: Future<Output = std::result::Result<T, reqwest::Error>>,
{
    if !crate::util::is_local_endpoint_url(request_url) {
        return operation().await;
    }

    let rewritten_url = crate::runtime::rewrite_url_for_logs(request_url);
    let mut attempt = 0_u32;
    let mut backoff = local_connect_retry_backoff();

    loop {
        attempt += 1;
        tracing::debug!(
            target: "vex::http",
            request_id = %request_id,
            url = %rewritten_url,
            operation = operation_name,
            attempt,
            "sending local request attempt"
        );

        match operation().await {
            Ok(result) => {
                if attempt > 1 {
                    tracing::info!(
                        target: "vex::http",
                        request_id = %request_id,
                        url = %rewritten_url,
                        operation = operation_name,
                        attempts = attempt,
                        "local API endpoint became available after startup retries"
                    );
                }
                return Ok(result);
            }
            Err(error) if should_retry_local_startup_error(&error) => {
                let Some(delay) = backoff.next_backoff() else {
                    return Err(error);
                };
                let next_delay_ms = delay.as_millis() as u64;
                tracing::warn!(
                    target: "vex::http",
                    request_id = %request_id,
                    url = %rewritten_url,
                    operation = operation_name,
                    error = %error,
                    next_delay_ms,
                    "local API endpoint not ready; retrying startup request failure with backoff"
                );
                tokio::time::sleep(delay).await;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests;
