//! Shared outbound HTTP client profile for workspace-owned transport seams.
//!
//! # Referenced Specifications
//!
//! | RFC | Title | Covered |
//! |-----|-------|---------|
//! | [RFC 9113](https://www.rfc-editor.org/rfc/rfc9113) | HTTP/2 | `default_client_builder`, `default_client` |

use anyhow::{Context, Result};
use reqwest::{Client, ClientBuilder};
use std::time::Duration;

/// Build the repository's default outbound HTTP client profile.
pub fn default_client_builder(skip_tls_verification: bool) -> ClientBuilder {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .pool_idle_timeout(Duration::from_secs(90))
        .read_timeout(Duration::from_secs(120))
        .tcp_keepalive(Duration::from_secs(60))
        .http2_adaptive_window(true)
        .http2_keep_alive_interval(Duration::from_secs(30))
        .http2_keep_alive_timeout(Duration::from_secs(10))
        .http2_keep_alive_while_idle(true)
        .user_agent(format!(
            "vexcoder/{} (+https://github.com/aistar-au/vexcoder)",
            env!("CARGO_PKG_VERSION")
        ))
        .danger_accept_invalid_certs(skip_tls_verification)
}

/// Build the repository's default outbound HTTP client.
pub fn default_client(skip_tls_verification: bool) -> Result<Client> {
    default_client_builder(skip_tls_verification)
        .build()
        .context("failed to build reqwest client")
}
