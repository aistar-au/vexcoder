use anyhow::{Context, Result};
use reqwest::{Client, ClientBuilder};
use std::time::Duration;

const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(120);
const LOCAL_MODEL_READ_TIMEOUT: Duration = Duration::from_secs(600);

pub fn default_client_builder(skip_tls_verification: bool) -> ClientBuilder {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .pool_idle_timeout(Duration::from_secs(90))
        .read_timeout(DEFAULT_READ_TIMEOUT)
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

pub fn default_client(skip_tls_verification: bool) -> Result<Client> {
    default_client_builder(skip_tls_verification)
        .build()
        .context("failed to build reqwest client")
}

pub fn model_client(endpoint_url: &str, skip_tls_verification: bool) -> Result<Client> {
    let builder = if crate::util::is_local_endpoint_url(endpoint_url) {
        default_client_builder(skip_tls_verification).read_timeout(LOCAL_MODEL_READ_TIMEOUT)
    } else {
        default_client_builder(skip_tls_verification)
    };

    builder.build().context("failed to build reqwest client")
}
