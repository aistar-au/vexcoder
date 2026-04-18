use anyhow::{Context, Result};
use axum::extract::DefaultBodyLimit;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as HyperConnectionBuilder;
use hyper_util::service::TowerToHyperService;
use std::sync::Arc;
use std::time::Duration;
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use tower_http::trace::TraceLayer;

use super::handlers::{
    agents_handler, approve_handler, delegate_handler, get_session_task_handler, health_handler,
    interrupt_handler, join_status_handler, list_session_tasks_handler, list_tasks_handler,
    list_todos_handler, post_peer_message_handler, projection_handler, read_peer_messages_handler,
    release_session_task_handler, schedule_team_handler, schema_handler, task_graph_handler,
    turns_handler, update_session_task_status_handler, watch_handler, watch_session_task_handler,
};
use super::{HSTS_HEADER_VALUE, HttpSurfaceSettings, ProblemDetails, ResolvedHttpSurface};
use crate::app::runtime_tokio::{net::TcpListener, select, spawn};
#[cfg(test)]
use crate::config::Config;
use crate::http_facade::{HeaderValue, Request, StatusCode, header};
use crate::local_api::LocalApiState;

#[cfg(test)]
pub fn build_router(config: Config) -> Router {
    build_router_with_state(LocalApiState::new(config))
}

pub fn build_router_with_state(state: LocalApiState) -> Router {
    Router::new()
        .route("/v1/health", get(health_handler))
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
    build_router_with_state(state)
        .layer(middleware::from_fn(move |request, next| {
            let expected_header = Arc::clone(&expected_header);
            async move {
                authorize_http_request(request, next, expected_header, hsts_enabled).await
            }
        }))
        .layer(TraceLayer::new_for_http())
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
    use axum::response::IntoResponse;
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(ProblemDetails {
            r#type: "https://aistar-au.github.io/vexcoder/problems/unauthorized".to_string(),
            title: "unauthorized".to_string(),
            status: StatusCode::UNAUTHORIZED.as_u16(),
            detail: Some("unauthorized".to_string()),
            instance: None,
        }),
    )
        .into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/problem+json"),
    );
    response
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
                                eprintln!("[local api] connection error: {error}");
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
                            eprintln!("[local api] tls accept failed: {error}");
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
                                eprintln!("[local api] tls connection error: {error}");
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
