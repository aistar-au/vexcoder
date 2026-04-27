use super::{LOCAL_NON_STREAM_FALLBACK_TIMEOUT, protocol_name, retry_local_connect_errors};
use crate::api::client::{api_request_timeout_error, map_api_request_error, map_api_status_error};
use crate::api::logging::{debug_payload_enabled, emit_log_value};
use crate::api::stream::StreamParser;
use crate::runtime::backend::SignalStream;
use crate::runtime::{ModelProtocol, RuntimeEnvelope};
use anyhow::{Context, Result, bail};
use futures::stream;
use serde_json::{Value, json};
use std::time::Instant;

pub(super) async fn create_non_stream_fallback_stream(
    http: reqwest::Client,
    request_url: &str,
    payload: &serde_json::Value,
    headers: &reqwest::header::HeaderMap,
    protocol: ModelProtocol,
    request_id: &str,
    fallback_reason: &'static str,
) -> Result<SignalStream> {
    let fallback_start = Instant::now();
    let request_url_for_logs = crate::runtime::rewrite_url_for_logs(request_url);
    let is_local_endpoint = crate::util::is_local_endpoint_url(request_url);
    let local_timeout_ms = if is_local_endpoint {
        LOCAL_NON_STREAM_FALLBACK_TIMEOUT.as_millis() as u64
    } else {
        0
    };
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
        local_endpoint = is_local_endpoint,
        local_timeout_ms,
        "issuing non-streaming fallback request"
    );
    if debug_payload_enabled() {
        emit_log_value(&json!({
            "operation": "api.non_stream_fallback_request",
            "request_id": request_id,
            "url": request_url_for_logs,
            "protocol": protocol_name(protocol),
            "fallback_reason": fallback_reason,
            "local_endpoint": is_local_endpoint,
            "local_timeout_ms": local_timeout_ms,
        }));
    }

    let request_future =
        retry_local_connect_errors(request_url, request_id, "non_stream_fallback", || {
            let http = http.clone();
            let request_headers = request_headers.clone();
            let fallback_payload = fallback_payload.clone();
            let request_url = request_url.to_string();
            async move {
                let response = http
                    .post(&request_url)
                    .headers(request_headers)
                    .json(&fallback_payload)
                    .send()
                    .await?;
                let status = response.status();
                let retry_after = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                let body = response.text().await.unwrap_or_else(|error| {
                    format!("<failed to read non-streaming response body: {error}>")
                });
                Ok((status, retry_after, body))
            }
        });

    let (status, retry_after, body) = if is_local_endpoint {
        match tokio::time::timeout(LOCAL_NON_STREAM_FALLBACK_TIMEOUT, request_future).await {
            Ok(result) => result.map_err(|error| map_api_request_error(error, request_url))?,
            Err(_) => {
                return Err(api_request_timeout_error(
                    request_url,
                    &format!(
                        "local non-stream fallback exceeded {} ms",
                        LOCAL_NON_STREAM_FALLBACK_TIMEOUT.as_millis()
                    ),
                ));
            }
        }
    } else {
        request_future
            .await
            .map_err(|error| map_api_request_error(error, request_url))?
    };

    let latency_ms = fallback_start.elapsed().as_millis() as u64;

    tracing::info!(
        target: "vex::http",
        request_id = %request_id,
        url = %request_url_for_logs,
        status = status.as_u16(),
        fallback_reason = fallback_reason,
        latency_ms,
        body_bytes = body.len(),
        retry_after = retry_after.as_deref().unwrap_or("none"),
        "received non-streaming fallback response"
    );
    if debug_payload_enabled() {
        emit_log_value(&json!({
            "operation": "api.non_stream_fallback_response",
            "request_id": request_id,
            "url": request_url_for_logs,
            "protocol": protocol_name(protocol),
            "fallback_reason": fallback_reason,
            "status": status.as_u16(),
            "latency_ms": latency_ms,
            "body_bytes": body.len(),
            "retry_after": retry_after,
        }));
    }

    if !status.is_success() {
        return Err(map_api_status_error(
            status,
            &body,
            request_url,
            retry_after.as_deref(),
        ));
    }

    let envelopes =
        normalize_non_stream_response(protocol, &body, request_id, &request_url_for_logs)
            .with_context(|| {
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
    request_url_for_logs: &str,
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
    log_non_stream_response_shape(protocol, &response, request_id, request_url_for_logs);
    let mut parser = StreamParser::new();
    let mut envelopes = Vec::new();

    match protocol {
        ModelProtocol::MessagesV1 => {
            let content_blocks = messages_v1_content_blocks(&response);
            inject_synthetic_signal(
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
                inject_synthetic_signal(
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

            inject_synthetic_signal(
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
            let source_choices = require_chat_compat_choices(&response)?;
            let choices = source_choices
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
                .collect::<Vec<_>>();

            inject_synthetic_signal(
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

fn require_chat_compat_choices(response: &Value) -> Result<&Vec<Value>> {
    let Some(choices) = response.get("choices").and_then(Value::as_array) else {
        if response.get("error").is_some() {
            bail!("chat-compat fallback returned an error payload instead of completion choices");
        }
        bail!("chat-compat fallback response missing choices array");
    };

    if choices.is_empty() {
        bail!("chat-compat fallback response contained no choices");
    }

    Ok(choices)
}

fn log_non_stream_response_shape(
    protocol: ModelProtocol,
    response: &Value,
    request_id: &str,
    request_url_for_logs: &str,
) {
    match protocol {
        ModelProtocol::MessagesV1 => {
            let content_blocks = messages_v1_content_blocks(response);
            let text_block_count = content_blocks
                .iter()
                .filter(|block| {
                    block.get("type").and_then(Value::as_str) == Some("text")
                        && block
                            .get("text")
                            .and_then(Value::as_str)
                            .is_some_and(|text| !text.is_empty())
                })
                .count();
            let empty_text_block_count = content_blocks
                .iter()
                .filter(|block| {
                    block.get("type").and_then(Value::as_str) == Some("text")
                        && block.get("text").and_then(Value::as_str) == Some("")
                })
                .count();
            let tool_use_count = content_blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
                .count();
            let tool_use_input_object_count = content_blocks
                .iter()
                .filter(|block| {
                    block.get("type").and_then(Value::as_str) == Some("tool_use")
                        && matches!(block.get("input"), Some(Value::Object(_)))
                })
                .count();
            let tool_use_input_non_object_count = content_blocks
                .iter()
                .filter(|block| {
                    block.get("type").and_then(Value::as_str) == Some("tool_use")
                        && block.get("input").is_some()
                        && !matches!(block.get("input"), Some(Value::Object(_)))
                })
                .count();

            tracing::debug!(
                target: "vex::protocol",
                request_id = %request_id,
                protocol = protocol_name(protocol),
                content_block_count = content_blocks.len(),
                text_block_count,
                empty_text_block_count,
                tool_use_count,
                tool_use_input_object_count,
                tool_use_input_non_object_count,
                stop_reason = response
                    .get("stop_reason")
                    .and_then(|value| value.as_str())
                    .unwrap_or("none"),
                "inspected non-stream fallback payload shape"
            );
            if debug_payload_enabled() {
                emit_log_value(&json!({
                    "operation": "api.non_stream_fallback_shape",
                    "request_id": request_id,
                    "url": request_url_for_logs,
                    "protocol": protocol_name(protocol),
                    "content_block_count": content_blocks.len(),
                    "text_block_count": text_block_count,
                    "empty_text_block_count": empty_text_block_count,
                    "tool_use_count": tool_use_count,
                    "tool_use_input_object_count": tool_use_input_object_count,
                    "tool_use_input_non_object_count": tool_use_input_non_object_count,
                    "stop_reason": response
                        .get("stop_reason")
                        .and_then(Value::as_str)
                        .unwrap_or("none"),
                }));
            }
        }
        ModelProtocol::ChatCompat => {
            let choices = response
                .get("choices")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let text_choice_count = choices
                .iter()
                .filter(|choice| {
                    choice
                        .get("message")
                        .and_then(|message| message.get("content"))
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.is_empty())
                })
                .count();
            let reasoning_choice_count = choices
                .iter()
                .filter(|choice| {
                    choice
                        .get("message")
                        .and_then(|message| {
                            message
                                .get("reasoning_content")
                                .or_else(|| message.get("thinking"))
                        })
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.is_empty())
                })
                .count();
            let tool_call_count: usize = choices
                .iter()
                .map(|choice| {
                    choice
                        .get("message")
                        .and_then(|message| message.get("tool_calls"))
                        .and_then(Value::as_array)
                        .map(|tool_calls| tool_calls.len())
                        .unwrap_or_default()
                })
                .sum();
            let mut tool_argument_string_count = 0;
            let mut tool_argument_object_count = 0;
            let mut tool_argument_array_count = 0;
            let mut tool_argument_scalar_count = 0;
            let mut tool_argument_missing_count = 0;
            for choice in &choices {
                let Some(tool_calls) = choice
                    .get("message")
                    .and_then(|message| message.get("tool_calls"))
                    .and_then(Value::as_array)
                else {
                    continue;
                };
                for tool_call in tool_calls {
                    match tool_call
                        .get("function")
                        .and_then(|function| function.get("arguments"))
                    {
                        Some(Value::String(_)) => tool_argument_string_count += 1,
                        Some(Value::Object(_)) => tool_argument_object_count += 1,
                        Some(Value::Array(_)) => tool_argument_array_count += 1,
                        Some(_) => tool_argument_scalar_count += 1,
                        None => tool_argument_missing_count += 1,
                    }
                }
            }
            let null_content_choice_count = choices
                .iter()
                .filter(|choice| {
                    choice
                        .get("message")
                        .and_then(|message| message.get("content"))
                        .is_some_and(Value::is_null)
                })
                .count();
            let empty_content_choice_count = choices
                .iter()
                .filter(|choice| {
                    choice
                        .get("message")
                        .and_then(|message| message.get("content"))
                        .and_then(Value::as_str)
                        == Some("")
                })
                .count();
            let missing_content_choice_count = choices
                .iter()
                .filter(|choice| {
                    choice
                        .get("message")
                        .is_some_and(|message| message.get("content").is_none())
                })
                .count();
            let tool_only_choice_count = choices
                .iter()
                .filter(|choice| {
                    let message = choice.get("message");
                    let has_text = message
                        .and_then(|message| message.get("content"))
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.is_empty());
                    let has_reasoning = message
                        .and_then(|message| {
                            message
                                .get("reasoning_content")
                                .or_else(|| message.get("thinking"))
                        })
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.is_empty());
                    let has_tool_calls = message
                        .and_then(|message| message.get("tool_calls"))
                        .and_then(Value::as_array)
                        .is_some_and(|tool_calls| !tool_calls.is_empty());
                    !has_text && !has_reasoning && has_tool_calls
                })
                .count();
            let first_finish_reason = choices
                .first()
                .and_then(|choice| choice.get("finish_reason"))
                .and_then(Value::as_str)
                .unwrap_or("none");

            tracing::debug!(
                target: "vex::protocol",
                request_id = %request_id,
                protocol = protocol_name(protocol),
                choice_count = choices.len(),
                text_choice_count,
                reasoning_choice_count,
                tool_call_count,
                tool_argument_string_count,
                tool_argument_object_count,
                tool_argument_array_count,
                tool_argument_scalar_count,
                tool_argument_missing_count,
                null_content_choice_count,
                empty_content_choice_count,
                missing_content_choice_count,
                tool_only_choice_count,
                first_finish_reason,
                "inspected non-stream fallback payload shape"
            );
            if debug_payload_enabled() {
                emit_log_value(&json!({
                    "operation": "api.non_stream_fallback_shape",
                    "request_id": request_id,
                    "url": request_url_for_logs,
                    "protocol": protocol_name(protocol),
                    "choice_count": choices.len(),
                    "text_choice_count": text_choice_count,
                    "reasoning_choice_count": reasoning_choice_count,
                    "tool_call_count": tool_call_count,
                    "tool_argument_string_count": tool_argument_string_count,
                    "tool_argument_object_count": tool_argument_object_count,
                    "tool_argument_array_count": tool_argument_array_count,
                    "tool_argument_scalar_count": tool_argument_scalar_count,
                    "tool_argument_missing_count": tool_argument_missing_count,
                    "null_content_choice_count": null_content_choice_count,
                    "empty_content_choice_count": empty_content_choice_count,
                    "missing_content_choice_count": missing_content_choice_count,
                    "tool_only_choice_count": tool_only_choice_count,
                    "first_finish_reason": first_finish_reason,
                }));
            }
        }
    }
}

fn inject_synthetic_signal(
    parser: &mut StreamParser,
    envelopes: &mut Vec<RuntimeEnvelope>,
    frame_kind: &str,
    payload: Value,
) -> Result<()> {
    let payload = serde_json::to_string(&payload)?;
    envelopes.extend(parser.process_sse_frame(frame_kind, &payload)?);
    Ok(())
}
