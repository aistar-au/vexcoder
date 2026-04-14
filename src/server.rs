//! ADR-028 transport layer.
//!
//! This module owns the network and IPC transport wiring for the LocalApiServer
//! surface authorized by ADR-026.  It consumes the application facade
//! (`crate::app`) and frames ADR-025 `RuntimeEnvelope` events for
//! concrete transports (HTTP/SSE, Unix-domain socket).
//!
//! Transport code must not contain runtime logic, tool semantics, or
//! task orchestration.  It reaches the runtime exclusively through
//! facade-owned entrypoints.

pub mod handlers;
pub mod http;
pub mod socket;
pub mod sse;
#[cfg(test)]
mod tests;
pub mod util;

#[cfg(not(unix))]
use anyhow::bail;
use anyhow::{Result, anyhow};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::local_api::LocalApiState;

pub(crate) use self::http::build_http_router;
#[cfg(unix)]
pub(crate) use self::http::build_router_with_state;
pub(crate) use self::util::resolve_serve_config;

const HSTS_HEADER_VALUE: &str = "max-age=31536000";
const SSE_KEEPALIVE_TEXT: &str = "keepalive";
const SSE_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone, Debug)]
pub struct HttpSurfaceSettings {
    pub bearer_token: Arc<str>,
    pub hsts_enabled: bool,
}

#[derive(Debug)]
pub struct ResolvedServeConfig {
    pub http: Option<ResolvedHttpSurface>,
    pub unix: Option<ResolvedUnixSurface>,
}

#[derive(Debug)]
pub struct ResolvedHttpSurface {
    pub bind_addr: String,
    pub port: u16,
    pub auth: HttpSurfaceSettings,
    pub tls: Option<Arc<rustls::ServerConfig>>,
}

#[derive(Debug)]
#[cfg_attr(not(unix), allow(unused))]
pub struct ResolvedUnixSurface {
    pub socket_path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct ControlResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

pub async fn serve_local_api(
    config: Config,
    host_override: Option<String>,
    port_override: Option<u16>,
) -> Result<()> {
    let resolved = resolve_serve_config(&config, host_override, port_override)?;
    let state = LocalApiState::new(config);
    let shutdown = CancellationToken::new();
    let shutdown_signal = shutdown.clone();
    let signal_task = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            shutdown_signal.cancel();
        }
    });
    let mut servers = JoinSet::new();

    if let Some(http_surface) = resolved.http {
        let router = build_http_router(state.clone(), http_surface.auth.clone());
        servers.spawn(http::run_http_surface(
            router,
            http_surface,
            shutdown.clone(),
        ));
    }
    if let Some(unix) = resolved.unix {
        #[cfg(unix)]
        {
            let router = build_router_with_state(state.clone());
            servers.spawn(socket::run_unix_surface(router, unix, shutdown.clone()));
        }
        #[cfg(not(unix))]
        {
            let _ = unix;
            bail!("Unix-socket transport is only supported on macOS and Linux");
        }
    }

    let mut first_error: Option<anyhow::Error> = None;
    while let Some(result) = servers.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                first_error = Some(error);
                shutdown.cancel();
                break;
            }
            Err(error) => {
                first_error = Some(anyhow!("LocalApiServer task join error: {error}"));
                shutdown.cancel();
                break;
            }
        }
    }

    shutdown.cancel();
    while let Some(result) = servers.join_next().await {
        if let Ok(Err(error)) = result {
            first_error.get_or_insert(error);
        }
    }
    signal_task.abort();

    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(())
}
