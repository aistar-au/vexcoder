use anyhow::{Context, Result};
use axum::Router;
use axum::body::Body;
use axum::extract::DefaultBodyLimit;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as HyperConnectionBuilder;
use hyper_util::service::TowerToHyperService;
use opentelemetry::KeyValue;
use opentelemetry_semantic_conventions::attribute as semconv;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use tower_governor::governor::GovernorConfigBuilder;
use tower_governor::key_extractor::KeyExtractor;
use tower_governor::{GovernorError, GovernorLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::sensitive_headers::SetSensitiveRequestHeadersLayer;
use tower_http::trace::TraceLayer;

use super::handlers::{
    agents_handler, approve_handler, delegate_handler, get_session_task_handler, health_handler,
    interrupt_handler, join_status_handler, list_session_tasks_handler, list_tasks_handler,
    list_todos_handler, post_peer_message_handler, privacy_handler, projection_handler,
    read_peer_messages_handler, release_session_task_handler, schedule_team_handler,
    schema_handler, task_graph_handler, turns_handler, update_session_task_status_handler,
    watch_handler, watch_session_task_handler,
};
use super::util::ProblemDetailsResponse;
use super::{HSTS_HEADER_VALUE, HttpSurfaceSettings, ResolvedHttpSurface};
use crate::app::runtime_tokio::{net::TcpListener, select, spawn};
#[cfg(test)]
use crate::config::Config;
use crate::http_facade::{HeaderValue, Request, StatusCode, header};
use crate::local_api::LocalApiState;
use crate::observability::REQUEST_ID_HEADER;

const LOCAL_API_RATE_LIMIT_PER_SECOND: u64 = 120;
const LOCAL_API_RATE_LIMIT_BURST: u32 = 240;

#[cfg(test)]
pub fn build_router(config: Config) -> Router {
    build_router_with_state(LocalApiState::new(config))
}

pub fn build_router_with_state(state: LocalApiState) -> Router {
    Router::new()
        .route("/v1/health", get(health_handler))
        .route("/v1/privacy", get(privacy_handler))
        .route("/v1/schema", get(schema_handler))
        .route("/v1/agents", get(agents_handler))
        .route("/v1/delegate", post(delegate_handler))
        .route("/v1/turns", post(turns_handler))
        .route("/v1/watch/{id}", get(watch_handler))
        .route(
            "/v1/session-tasks/{id}/release",
            post(release_session_task_handler),
        )
        .route(
            "/v1/teams/{team_name}/schedule",
            post(schedule_team_handler),
        )
        .route("/v1/tasks/{task_id}/join-status", get(join_status_handler))
        .route("/v1/tasks", get(list_tasks_handler))
        .route("/v1/session-tasks", get(list_session_tasks_handler))
        .route("/v1/session-tasks/{id}", get(get_session_task_handler))
        .route(
            "/v1/session-tasks/{id}/status",
            patch(update_session_task_status_handler),
        )
        .route(
            "/v1/session-tasks/{id}/watch",
            get(watch_session_task_handler),
        )
        .route("/v1/interrupt", post(interrupt_handler))
        .route("/v1/approve", post(approve_handler))
        .route("/v1/task-graph", get(task_graph_handler))
        .route("/v1/todos", get(list_todos_handler))
        .route("/v1/projection", get(projection_handler))
        .route(
            "/v1/tasks/{parent_task_id}/messages",
            post(post_peer_message_handler).get(read_peer_messages_handler),
        )
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(state)
}

pub fn build_http_router(state: LocalApiState, auth: HttpSurfaceSettings) -> Router {
    let expected_header = Arc::<str>::from(format!("Bearer {}", auth.bearer_token));
    let hsts_enabled = auth.hsts_enabled;
    let mut governor = GovernorConfigBuilder::default().key_extractor(AuthorizedClientKeyExtractor);
    governor.per_second(LOCAL_API_RATE_LIMIT_PER_SECOND);
    governor.burst_size(LOCAL_API_RATE_LIMIT_BURST);
    let governor = governor
        .use_headers()
        .finish()
        .expect("local API governor config must be valid");
    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &Request<_>| {
            let request_id = request
                .headers()
                .get(REQUEST_ID_HEADER)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("missing");
            let traceparent = request
                .headers()
                .get("traceparent")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("missing");
            let span = tracing::info_span!(
                "local_api_http_request",
                method = %request.method(),
                path = %request.uri().path(),
                request_id = %request_id,
                traceparent = %traceparent,
                trace_id = tracing::field::Empty,
                otel_span_id = tracing::field::Empty,
            );
            crate::observability::attach_standard_baggage(
                &span,
                "local_api",
                "http_request",
                Some(request_id),
                None,
            );
            let mut attributes = vec![KeyValue::new(
                semconv::HTTP_REQUEST_METHOD,
                request.method().as_str().to_string(),
            )];
            if let Some((server_address, server_port)) = request_host_parts(request) {
                attributes.push(KeyValue::new(semconv::SERVER_ADDRESS, server_address));
                if let Some(server_port) = server_port {
                    attributes.push(KeyValue::new(semconv::SERVER_PORT, i64::from(server_port)));
                }
            }
            crate::observability::set_span_attributes(&span, attributes);
            span
        })
        .on_response(
            |response: &Response, latency: Duration, span: &tracing::Span| {
                tracing::info!(
                    parent: span,
                    status = response.status().as_u16(),
                    latency_ms = latency.as_millis() as u64,
                    "completed local API request"
                );
            },
        )
        .on_failure(|error, latency: Duration, span: &tracing::Span| {
            tracing::warn!(
                parent: span,
                latency_ms = latency.as_millis() as u64,
                failure = ?error,
                "local API request failed"
            );
        });
    build_router_with_state(state)
        .layer(GovernorLayer::new(governor).error_handler(governor_error_response))
        .layer(middleware::from_fn(move |request, next| {
            let expected_header = Arc::clone(&expected_header);
            async move {
                authorize_http_request(request, next, expected_header, hsts_enabled).await
            }
        }))
        .layer(trace_layer)
        .layer(SetSensitiveRequestHeadersLayer::new(std::iter::once(
            header::AUTHORIZATION,
        )))
        .layer(OtelAxumLayer::default())
        .layer(OtelInResponseLayer)
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
}

#[derive(Clone, Copy, Debug, Default)]
struct AuthorizedClientKeyExtractor;

impl KeyExtractor for AuthorizedClientKeyExtractor {
    type Key = String;

    fn name(&self) -> &'static str {
        "authorized client"
    }

    fn extract<T>(&self, req: &http::Request<T>) -> Result<Self::Key, GovernorError> {
        if let Some(ip) = forwarded_client_ip(req.headers()) {
            return Ok(format!("ip:{ip}"));
        }

        authorized_header_hash(req.headers())
            .map(|hash| format!("auth:{hash:016x}"))
            .ok_or(GovernorError::UnableToExtractKey)
    }
}

fn forwarded_client_ip(headers: &http::HeaderMap) -> Option<std::net::IpAddr> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .split(',')
                .find_map(|segment| segment.trim().parse::<std::net::IpAddr>().ok())
        })
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<std::net::IpAddr>().ok())
        })
}

fn authorized_header_hash(headers: &http::HeaderMap) -> Option<u64> {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())?;
    let mut hasher = DefaultHasher::new();
    authorization.hash(&mut hasher);
    Some(hasher.finish())
}

fn request_host_parts<T>(request: &Request<T>) -> Option<(String, Option<u16>)> {
    let authority = request
        .headers()
        .get("host")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<http::uri::Authority>().ok())?;
    Some((authority.host().to_string(), authority.port_u16()))
}

fn governor_error_response(error: GovernorError) -> http::Response<Body> {
    let (status, reason, headers) = match error {
        GovernorError::TooManyRequests { wait_time, headers } => {
            tracing::warn!(
                target: "vex::http",
                wait_time_secs = wait_time,
                "local API rate limit exceeded"
            );
            (StatusCode::TOO_MANY_REQUESTS, "rate_limited", headers)
        }
        GovernorError::UnableToExtractKey => {
            tracing::error!(
                target: "vex::http",
                "local API rate limiter could not extract a key"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "rate_limit_key_unavailable",
                None,
            )
        }
        GovernorError::Other { code, msg, headers } => {
            tracing::warn!(
                target: "vex::http",
                status = code.as_u16(),
                message = msg.as_deref().unwrap_or("unknown"),
                "local API rate limiter returned a custom error"
            );
            (
                StatusCode::from_u16(code.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                "rate_limit_error",
                headers,
            )
        }
    };

    let mut response = ProblemDetailsResponse::from_reason(status, reason).into_response();
    if let Some(headers) = headers {
        response.headers_mut().extend(headers);
    }
    response
}

async fn authorize_http_request(
    request: Request,
    next: Next,
    expected_header: Arc<str>,
    hsts_enabled: bool,
) -> Response {
    let provided = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim);
    if provided != Some(expected_header.as_ref()) {
        return unauthorized_response();
    }

    let mut response = next.run(request).await;
    if hsts_enabled {
        response.headers_mut().insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static(HSTS_HEADER_VALUE),
        );
    }
    response
}

fn unauthorized_response() -> Response {
    ProblemDetailsResponse::from_reason(StatusCode::UNAUTHORIZED, "unauthorized").into_response()
}

pub async fn run_http_surface(
    router: Router,
    surface: ResolvedHttpSurface,
    shutdown: CancellationToken,
) -> Result<()> {
    let listener = TcpListener::bind((surface.bind_addr.as_str(), surface.port))
        .await
        .with_context(|| {
            format!(
                "failed to bind LocalApiServer on {}:{}",
                surface.bind_addr, surface.port
            )
        })?;

    match surface.tls {
        Some(tls_config) => serve_tls_listener(listener, router, tls_config, shutdown).await,
        None => serve_plain_listener(listener, router, shutdown).await,
    }
}

async fn serve_plain_listener(
    listener: TcpListener,
    router: Router,
    shutdown: CancellationToken,
) -> Result<()> {
    loop {
        select! {
            _ = shutdown.cancelled() => return Ok(()),
            accepted = listener.accept() => {
                let (stream, _) = accepted.context("failed to accept LocalApiServer connection")?;
                let service = TowerToHyperService::new(router.clone());
                let shutdown = shutdown.clone();
                spawn(async move {
                    let io = TokioIo::new(stream);
                    let builder = http1_connection_builder();
                    let connection = builder.serve_connection(io, service);
                    select! {
                        _ = shutdown.cancelled() => {}
                        result = connection => {
                            if let Err(error) = result {
                                tracing::warn!(
                                    target: "vex::http",
                                    error = %error,
                                    "local API connection error"
                                );
                            }
                        }
                    }
                });
            }
        }
    }
}

async fn serve_tls_listener(
    listener: TcpListener,
    router: Router,
    tls_config: Arc<rustls::ServerConfig>,
    shutdown: CancellationToken,
) -> Result<()> {
    let acceptor = TlsAcceptor::from(tls_config);
    loop {
        select! {
            _ = shutdown.cancelled() => return Ok(()),
            accepted = listener.accept() => {
                let (stream, _) = accepted.context("failed to accept LocalApiServer TLS connection")?;
                let acceptor = acceptor.clone();
                let service = TowerToHyperService::new(router.clone());
                let shutdown = shutdown.clone();
                spawn(async move {
                    let tls_stream = match acceptor.accept(stream).await {
                        Ok(stream) => stream,
                        Err(error) => {
                            tracing::warn!(
                                target: "vex::http",
                                error = %error,
                                "local API TLS accept failed"
                            );
                            return;
                        }
                    };
                    let io = TokioIo::new(tls_stream);
                    let builder = http1_connection_builder();
                    let connection = builder.serve_connection(io, service);
                    select! {
                        _ = shutdown.cancelled() => {}
                        result = connection => {
                            if let Err(error) = result {
                                tracing::warn!(
                                    target: "vex::http",
                                    error = %error,
                                    "local API TLS connection error"
                                );
                            }
                        }
                    }
                });
            }
        }
    }
}

fn http1_connection_builder() -> HyperConnectionBuilder<TokioExecutor> {
    let mut builder = HyperConnectionBuilder::new(TokioExecutor::new());
    builder.http1().header_read_timeout(Duration::from_secs(30));
    builder.http1().keep_alive(true);
    builder
}
