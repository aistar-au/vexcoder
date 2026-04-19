//! Client-side protocol discovery for local inference endpoints.
//!
//! Users configure only `base_url` (e.g. `http://127.0.0.1:8000`).
//! [`discover_protocol`] probes the server once at connection time and
//! returns a cached [`ModelProtocol`] for the session.
//!
//! Two probes are attempted in order:
//! 1. `GET /v1/messages` with `Accept: text/event-stream`
//! 2. `GET /v1/chat/completions` with `Accept: text/event-stream`
//!
//! The first probe that returns `200 OK` with a `text/event-stream`
//! `Content-Type` wins. If both fail, [`DiscoveryError::AllProbesFailed`]
//! is returned with per-probe diagnostics for debugging.
//!
//! ADR-047 §6 — Client-Side Protocol Discovery.

use crate::runtime::backend::ModelProtocol;
use crate::runtime::rewrite_url_for_logs;
use std::time::{Duration, Instant};

/// Result of a successful protocol probe.
#[derive(Debug, Clone)]
pub struct DiscoveryResult {
    /// The accepted protocol confirmed by the probe.
    pub protocol: ModelProtocol,
    /// The full endpoint URL that responded (e.g. `http://…/v1/messages`).
    pub endpoint: String,
    /// Round-trip latency of the winning probe in milliseconds.
    pub probe_latency_ms: u64,
}

/// Diagnostic record for a single probe attempt.
///
/// Included in [`DiscoveryError::AllProbesFailed`] so callers can surface
/// meaningful error messages without re-running the probes.
#[derive(Debug, Clone)]
pub struct ProbeAttempt {
    /// The URL that was probed.
    pub endpoint: String,
    /// The `Accept` header sent in the probe request.
    pub accept_header: String,
    /// HTTP status code, or `None` if the request could not be sent.
    pub status: Option<u16>,
    /// Error description if the probe failed.
    pub error: Option<String>,
    /// Round-trip latency in milliseconds.
    pub latency_ms: u64,
}

/// Errors returned by [`discover_protocol`].
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    /// Neither the messages-v1 nor the chat-compat probe succeeded.
    ///
    /// `attempts` contains one entry per probe with status and error details.
    #[error("all protocol probes failed; see attempts for details")]
    AllProbesFailed {
        /// Ordered list of probe attempts (messages-v1 first, then
        /// chat-compat).
        attempts: Vec<ProbeAttempt>,
    },
    /// A network-level error prevented the probe from completing.
    #[error("network error during protocol probe: {0}")]
    NetworkError(String),
}

/// Probe a single endpoint and return a [`ProbeAttempt`].
///
/// A probe is considered successful when the server returns `200 OK` and
/// a `Content-Type` that contains `text/event-stream`.
async fn probe_endpoint(
    url: &str,
    accept_header: &str,
    timeout: Duration,
    client: &reqwest::Client,
) -> ProbeAttempt {
    let start = Instant::now();

    tracing::debug!(
        target: "vex::http",
        method = "GET",
        url = %rewrite_url_for_logs(url),
        accept = accept_header,
        timeout_ms = timeout.as_millis() as u64,
        "sending protocol discovery probe"
    );

    let response = client
        .get(url)
        .header("Accept", accept_header)
        .header("Accept-Encoding", "identity")
        .timeout(timeout)
        .send()
        .await;

    let latency_ms = start.elapsed().as_millis() as u64;

    match response {
        Ok(resp) => {
            let status = resp.status().as_u16();
            if status == 200 {
                let is_sse = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .map(|ct| ct.contains("text/event-stream"))
                    .unwrap_or(false);

                if is_sse {
                    return ProbeAttempt {
                        endpoint: url.to_string(),
                        accept_header: accept_header.to_string(),
                        status: Some(status),
                        error: None,
                        latency_ms,
                    };
                }
            }

            ProbeAttempt {
                endpoint: url.to_string(),
                accept_header: accept_header.to_string(),
                status: Some(status),
                error: Some("server did not return text/event-stream".to_string()),
                latency_ms,
            }
        }
        Err(e) => ProbeAttempt {
            endpoint: url.to_string(),
            accept_header: accept_header.to_string(),
            status: None,
            error: Some(e.to_string()),
            latency_ms,
        },
    }
}

/// Discover the streaming protocol supported by a local inference
/// server at `base_url`.
///
/// Probes are performed in order: messages-v1 first, then chat-compat.
/// The first successful probe terminates the sequence.
///
/// # Errors
///
/// Returns [`DiscoveryError::AllProbesFailed`] with per-probe diagnostics
/// when neither endpoint responds as expected.
pub async fn discover_protocol(
    base_url: &str,
    client: &reqwest::Client,
    timeout: Duration,
) -> Result<DiscoveryResult, DiscoveryError> {
    let base = base_url.trim_end_matches('/');
    let mut attempts = Vec::with_capacity(2);

    let block_probe = probe_endpoint(
        &format!("{base}/v1/messages"),
        "text/event-stream",
        timeout,
        client,
    )
    .await;

    let block_ok = block_probe.status == Some(200) && block_probe.error.is_none();
    let block_endpoint = block_probe.endpoint.clone();
    let block_latency = block_probe.latency_ms;
    attempts.push(block_probe);

    if block_ok {
        return Ok(DiscoveryResult {
            protocol: ModelProtocol::MessagesV1,
            endpoint: block_endpoint,
            probe_latency_ms: block_latency,
        });
    }

    let choices_probe = probe_endpoint(
        &format!("{base}/v1/chat/completions"),
        "text/event-stream",
        timeout,
        client,
    )
    .await;

    let choices_ok = choices_probe.status == Some(200) && choices_probe.error.is_none();
    let choices_endpoint = choices_probe.endpoint.clone();
    let choices_latency = choices_probe.latency_ms;
    attempts.push(choices_probe);

    if choices_ok {
        return Ok(DiscoveryResult {
            protocol: ModelProtocol::ChatCompat,
            endpoint: choices_endpoint,
            probe_latency_ms: choices_latency,
        });
    }

    Err(DiscoveryError::AllProbesFailed { attempts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_protocol_eq() {
        assert_eq!(ModelProtocol::MessagesV1, ModelProtocol::MessagesV1);
        assert_ne!(ModelProtocol::MessagesV1, ModelProtocol::ChatCompat);
    }

    #[test]
    fn discovery_error_display() {
        let err = DiscoveryError::AllProbesFailed { attempts: vec![] };
        assert!(err.to_string().contains("all protocol probes failed"));

        let err = DiscoveryError::NetworkError("connection refused".to_string());
        assert!(err.to_string().contains("connection refused"));
    }

    #[test]
    fn probe_attempt_fields() {
        let attempt = ProbeAttempt {
            endpoint: "http://127.0.0.1:8000/v1/messages".to_string(),
            accept_header: "text/event-stream".to_string(),
            status: Some(200),
            error: None,
            latency_ms: 12,
        };
        assert_eq!(attempt.status, Some(200));
        assert!(attempt.error.is_none());
    }
}
