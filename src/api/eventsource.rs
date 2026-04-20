//! Reqwest-backed EventSource bridge.
//!
//! The upstream streaming endpoints require `POST` request bodies, so this
//! module intentionally uses EventSource framing without the browser
//! `GET`-only interface contract. Reconnect is disabled because retrying a
//! non-idempotent streamed generation request would duplicate work and billing.

use crate::api::client::{map_api_request_error, map_api_status_error};
use crate::api::stream::StreamParser;
use crate::runtime::backend::EventStream;
use crate::runtime::{ModelProtocol, RuntimeEnvelope, RuntimeEvent, TokenUsageEnvelope};
use anyhow::{Context, Result, anyhow};
use backoff::ExponentialBackoffBuilder;
use backoff::backoff::Backoff;
use bytes::Bytes;
use eventsource_client::{Client as _, ClientBuilder, ReconnectOptions, SSE};
use futures::{StreamExt, stream};
use launchdarkly_sdk_transport::{
    ByteStream as TransportByteStream, HttpTransport, ResponseFuture, TransportError,
};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::future::Future;
use std::time::{Duration, Instant};

#[cfg(test)]
const LOCAL_STREAM_START_TIMEOUT: Duration = Duration::from_millis(50);
#[cfg(not(test))]
const LOCAL_STREAM_START_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const LOCAL_CONNECT_RETRY_MAX_ELAPSED: Duration = Duration::from_millis(250);
#[cfg(not(test))]
const LOCAL_CONNECT_RETRY_MAX_ELAPSED: Duration = Duration::from_secs(2);
const LOCAL_CONNECT_RETRY_INITIAL_INTERVAL: Duration = Duration::from_millis(100);
const LOCAL_CONNECT_RETRY_MAX_INTERVAL: Duration = Duration::from_millis(400);

pub(crate) async fn create_event_stream(
    http: reqwest::Client,
    request_url: &str,
    payload: &serde_json::Value,
    headers: &reqwest::header::HeaderMap,
    protocol: ModelProtocol,
    request_id: &str,
) -> Result<EventStream> {
    let request_start = Instant::now();
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

    let client = builder.build_with_transport(ReqwestEventSourceTransport {
        client: http.clone(),
        request_id: request_id.to_string(),
    });
    let mut state = EventStreamState {
        upstream: client.stream(),
        parser: StreamParser::new(),
        pending: VecDeque::new(),
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
    };

    let first_event = if crate::util::is_local_endpoint_url(request_url) {
        match tokio::time::timeout(LOCAL_STREAM_START_TIMEOUT, next_stream_event(&mut state)).await
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
                return create_non_stream_fallback_stream(
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
        next_stream_event(&mut state).await?
    };
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

async fn create_non_stream_fallback_stream(
    http: reqwest::Client,
    request_url: &str,
    payload: &serde_json::Value,
    headers: &reqwest::header::HeaderMap,
    protocol: ModelProtocol,
    request_id: &str,
    fallback_reason: &'static str,
) -> Result<EventStream> {
    let fallback_start = Instant::now();
    let request_url_for_logs = crate::runtime::rewrite_url_for_logs(request_url);
    let mut request_headers = headers.clone();
    request_headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("application/json"),
    );
    let fallback_payload = non_stream_payload(payload);

    tracing::info!(
        target: "vex::http",
        request_id = %request_id,
        url = %request_url_for_logs,
        fallback_reason = fallback_reason,
        protocol = ?protocol,
        "issuing non-streaming fallback request"
    );

    let response =
        retry_local_connect_errors(request_url, request_id, "non_stream_fallback", || {
            let http = http.clone();
            let request_headers = request_headers.clone();
            let fallback_payload = fallback_payload.clone();
            async move {
                http.post(request_url)
                    .headers(request_headers)
                    .json(&fallback_payload)
                    .send()
                    .await
            }
        })
        .await
        .map_err(|error| map_api_request_error(error, request_url))?;

    let status = response.status();
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = response
        .text()
        .await
        .unwrap_or_else(|error| format!("<failed to read non-streaming response body: {error}>"));

    tracing::info!(
        target: "vex::http",
        request_id = %request_id,
        url = %request_url_for_logs,
        status = status.as_u16(),
        fallback_reason = fallback_reason,
        latency_ms = fallback_start.elapsed().as_millis() as u64,
        body_bytes = body.len(),
        retry_after = retry_after.as_deref().unwrap_or("none"),
        "received non-streaming fallback response"
    );

    if !status.is_success() {
        return Err(map_api_status_error(
            status,
            &body,
            request_url,
            retry_after.as_deref(),
        ));
    }

    let envelopes =
        normalize_non_stream_response(protocol, &body, request_id).with_context(|| {
            format!(
                "failed to normalize non-streaming fallback response from '{}'",
                request_url_for_logs
            )
        })?;

    Ok(Box::pin(stream::iter(envelopes.into_iter().map(Ok))))
}

fn non_stream_payload(payload: &serde_json::Value) -> serde_json::Value {
    let mut payload = payload.clone();
    if let Some(object) = payload.as_object_mut() {
        object.insert("stream".to_string(), Value::Bool(false));
    }
    payload
}

fn normalize_non_stream_response(
    protocol: ModelProtocol,
    body: &str,
    request_id: &str,
) -> Result<Vec<RuntimeEnvelope>> {
    tracing::debug!(
        target: "vex::protocol",
        request_id = %request_id,
        protocol = ?protocol,
        response_bytes = body.len(),
        "normalizing non-streaming fallback response"
    );
    let response: Value =
        serde_json::from_str(body).with_context(|| "response body was not valid JSON")?;
    let mut parser = StreamParser::new();
    let mut envelopes = Vec::new();

    match protocol {
        ModelProtocol::MessagesV1 => {
            let content_blocks = messages_v1_content_blocks(&response);
            feed_synthetic_event(
                &mut parser,
                &mut envelopes,
                "message_start",
                json!({
                    "type": "message_start",
                    "message": {
                        "id": response
                            .get("id")
                            .cloned()
                            .unwrap_or_else(|| json!("nonstream-message")),
                        "type": response
                            .get("type")
                            .cloned()
                            .unwrap_or_else(|| json!("message")),
                        "role": response
                            .get("role")
                            .cloned()
                            .unwrap_or_else(|| json!("assistant")),
                        "model": response
                            .get("model")
                            .cloned()
                            .unwrap_or_else(|| json!("unknown")),
                        "content": content_blocks,
                        "stop_reason": response
                            .get("stop_reason")
                            .cloned()
                            .unwrap_or(Value::Null),
                        "stop_sequence": response
                            .get("stop_sequence")
                            .cloned()
                            .unwrap_or(Value::Null),
                    }
                }),
            )?;

            for (index, block) in messages_v1_content_blocks(&response)
                .into_iter()
                .enumerate()
            {
                feed_synthetic_event(
                    &mut parser,
                    &mut envelopes,
                    "content_block_start",
                    json!({
                        "type": "content_block_start",
                        "index": index,
                        "content_block": block,
                    }),
                )?;
            }

            feed_synthetic_event(
                &mut parser,
                &mut envelopes,
                "message_delta",
                json!({
                    "type": "message_delta",
                    "delta": {
                        "stop_reason": response
                            .get("stop_reason")
                            .cloned()
                            .unwrap_or(Value::Null),
                        "stop_sequence": response
                            .get("stop_sequence")
                            .cloned()
                            .unwrap_or(Value::Null),
                    },
                    "usage": response.get("usage").cloned().unwrap_or(Value::Null),
                }),
            )?;
        }
        ModelProtocol::ChatCompat => {
            let choices = response
                .get("choices")
                .and_then(Value::as_array)
                .map(|choices| {
                    choices
                        .iter()
                        .enumerate()
                        .map(|(fallback_index, choice)| {
                            let message = choice.get("message").cloned().unwrap_or(Value::Null);
                            let mut delta = serde_json::Map::new();

                            if let Some(role) = message.get("role").cloned() {
                                delta.insert("role".to_string(), role);
                            }
                            if let Some(content) = message.get("content").cloned() {
                                delta.insert("content".to_string(), content);
                            }
                            if let Some(reasoning) = message.get("reasoning_content").cloned() {
                                delta.insert("reasoning_content".to_string(), reasoning);
                            } else if let Some(thinking) = message.get("thinking").cloned() {
                                delta.insert("thinking".to_string(), thinking);
                            }
                            if let Some(refusal) = message.get("refusal").cloned() {
                                delta.insert("refusal".to_string(), refusal);
                            }
                            if let Some(tool_calls) = message.get("tool_calls").cloned() {
                                delta.insert("tool_calls".to_string(), tool_calls);
                            }

                            json!({
                                "index": choice
                                    .get("index")
                                    .and_then(Value::as_u64)
                                    .unwrap_or(fallback_index as u64),
                                "delta": Value::Object(delta),
                                "finish_reason": choice
                                    .get("finish_reason")
                                    .cloned()
                                    .unwrap_or(Value::Null),
                                "logprobs": choice.get("logprobs").cloned().unwrap_or(Value::Null),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            feed_synthetic_event(
                &mut parser,
                &mut envelopes,
                "",
                json!({
                    "id": response.get("id").cloned().unwrap_or(Value::Null),
                    "object": response.get("object").cloned().unwrap_or_else(|| json!("chat.completion")),
                    "created": response.get("created").cloned().unwrap_or(Value::Null),
                    "model": response.get("model").cloned().unwrap_or(Value::Null),
                    "system_fingerprint": response
                        .get("system_fingerprint")
                        .cloned()
                        .unwrap_or(Value::Null),
                    "service_tier": response.get("service_tier").cloned().unwrap_or(Value::Null),
                    "choices": choices,
                    "usage": response.get("usage").cloned().unwrap_or(Value::Null),
                    "prompt_progress": response
                        .get("prompt_progress")
                        .cloned()
                        .unwrap_or(Value::Null),
                    "timings": response.get("timings").cloned().unwrap_or(Value::Null),
                }),
            )?;
        }
    }

    envelopes.extend(parser.finish());
    tracing::debug!(
        target: "vex::protocol",
        request_id = %request_id,
        protocol = ?protocol,
        envelope_count = envelopes.len(),
        "completed non-streaming fallback normalization"
    );
    Ok(envelopes)
}

fn messages_v1_content_blocks(response: &Value) -> Vec<Value> {
    match response.get("content") {
        Some(Value::Array(blocks)) => blocks.clone(),
        Some(Value::String(text)) => vec![json!({ "type": "text", "text": text })],
        _ => Vec::new(),
    }
}

fn feed_synthetic_event(
    parser: &mut StreamParser,
    envelopes: &mut Vec<RuntimeEnvelope>,
    event_type: &str,
    payload: Value,
) -> Result<()> {
    let payload = serde_json::to_string(&payload)?;
    envelopes.extend(parser.process_sse_event(event_type, &payload)?);
    Ok(())
}

struct EventStreamState {
    upstream: eventsource_client::BoxStream<eventsource_client::Result<SSE>>,
    parser: StreamParser,
    pending: VecDeque<RuntimeEnvelope>,
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

fn finish_pending_events(state: &mut EventStreamState) -> Option<RuntimeEnvelope> {
    state.pending.extend(state.parser.finish());
    state.pending.pop_front()
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

        let Some(item) = state.upstream.next().await else {
            if let Some(event) = finish_pending_events(state) {
                observe_envelope(state, &event);
                return Ok(Some(event));
            }
            emit_stream_summary(state, "stream_closed");
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
                    observe_envelope(state, &event);
                    return Ok(Some(event));
                }
                emit_stream_summary(state, "stream_eof");
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
                tracing::warn!(
                    target: "vex::http",
                    request_id = %state.request_id,
                    url = %crate::runtime::rewrite_url_for_logs(&state.request_url),
                    status = status.as_u16(),
                    body_bytes = body.len(),
                    "streaming endpoint returned an unexpected response"
                );
                emit_stream_summary(state, "unexpected_response");
                return Err(map_api_status_error(
                    status,
                    &body,
                    &state.request_url,
                    retry_after.as_deref(),
                ));
            }
            Err(eventsource_client::Error::Transport(error)) => {
                tracing::warn!(
                    target: "vex::http",
                    request_id = %state.request_id,
                    url = %crate::runtime::rewrite_url_for_logs(&state.request_url),
                    error = %error,
                    "streaming transport error"
                );
                emit_stream_summary(state, "transport_error");
                return Err(anyhow!(error.to_string()));
            }
            Err(eventsource_client::Error::TimedOut) => {
                tracing::warn!(
                    target: "vex::http",
                    request_id = %state.request_id,
                    url = %crate::runtime::rewrite_url_for_logs(&state.request_url),
                    "streaming request timed out"
                );
                emit_stream_summary(state, "stream_timeout");
                return Err(anyhow!("API request to '{}' timed out", state.request_url));
            }
            Err(error) => {
                emit_stream_summary(state, "stream_error");
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
    request_id: String,
}

impl HttpTransport for ReqwestEventSourceTransport {
    fn request(&self, request: http::Request<Option<Bytes>>) -> ResponseFuture {
        let client = self.client.clone();
        let request_id = self.request_id.clone();

        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let request_url = parts.uri.to_string();
            let revised_request_url = crate::runtime::rewrite_url_for_logs(&request_url);
            let method = parts.method.clone();
            let headers = parts.headers.clone();
            let request_body = body.clone();

            tracing::debug!(
                target: "vex::http",
                request_id = %request_id,
                method = %parts.method,
                url = %revised_request_url,
                "sending streaming request"
            );

            let response =
                retry_local_connect_errors(&request_url, &request_id, "streaming_request", || {
                    let client = client.clone();
                    let method = method.clone();
                    let headers = headers.clone();
                    let request_url = request_url.clone();
                    let request_body = request_body.clone();
                    async move {
                        let mut reqwest_request = client.request(method, request_url);
                        for (name, value) in &headers {
                            reqwest_request = reqwest_request.header(name, value);
                        }
                        if let Some(body) = request_body {
                            reqwest_request = reqwest_request.body(body);
                        }
                        reqwest_request.send().await
                    }
                })
                .await
                .map_err(|error| {
                    TransportError::new(std::io::Error::other(
                        map_api_request_error(error, &request_url).to_string(),
                    ))
                })?;

            let status = response.status();
            let headers = response.headers().clone();
            let request_url_for_stream = request_url.clone();
            tracing::debug!(
                target: "vex::http",
                request_id = %request_id,
                url = %revised_request_url,
                status = status.as_u16(),
                "streaming request connected"
            );
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

fn local_connect_retry_backoff() -> backoff::ExponentialBackoff {
    let mut builder = ExponentialBackoffBuilder::new();
    builder
        .with_initial_interval(LOCAL_CONNECT_RETRY_INITIAL_INTERVAL)
        .with_multiplier(2.0)
        .with_max_interval(LOCAL_CONNECT_RETRY_MAX_INTERVAL)
        .with_max_elapsed_time(Some(LOCAL_CONNECT_RETRY_MAX_ELAPSED));
    builder.build()
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
            Err(error) if error.is_connect() => {
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
                    "local API endpoint not ready; retrying connect failure with backoff"
                );
                tokio::time::sleep(delay).await;
            }
            Err(error) => return Err(error),
        }
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
            request_id: "req-test-eof".to_string(),
            protocol_name: "messages-v1",
            started_at: Instant::now(),
            envelope_count: 0,
            tool_call_starts: 0,
            tool_call_completions: 0,
            tool_call_failures: 0,
            last_usage: None,
            final_status: None,
            summary_emitted: false,
        };

        let event = next_stream_event(&mut state).await.unwrap().unwrap();

        assert!(matches!(event.event, RuntimeEvent::TurnEnd { .. }));
        assert!(next_stream_event(&mut state).await.unwrap().is_none());
    }
}
