//! SOCKS5 and HTTP proxy configuration for outbound connections.
//!
//! # Referenced Specifications
//!
//! | RFC | Title | Covered |
//! |-----|-------|---------|
//! | [RFC 1928](https://www.rfc-editor.org/rfc/rfc1928) | SOCKS Protocol Version 5 | `ProxyConfig::Socks5` |
//!
//! HTTP CONNECT proxying (RFC 7231 §4.3.6) is also supported via
//! `ProxyConfig::Http` and `ProxyConfig::Https`.  The `reqwest` crate
//! handles the SOCKS5 handshake (RFC 1928 §3–6) and HTTP CONNECT tunnel
//! negotiation transparently once the proxy URL is configured.

use anyhow::{Context, Result};
use reqwest::Proxy;

/// Proxy configuration for outbound HTTP/HTTPS connections.
///
/// Maps to the proxy URL schemes understood by `reqwest`:
/// - `socks5://` — RFC 1928 SOCKS5 without authentication
/// - `socks5h://` — RFC 1928 SOCKS5 with remote DNS resolution
/// - `http://` — HTTP CONNECT proxy (RFC 7231 §4.3.6)
/// - `https://` — HTTP CONNECT over TLS
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyConfig {
    /// SOCKS5 proxy with local DNS resolution (RFC 1928).
    Socks5 { host: String, port: u16 },
    /// SOCKS5 proxy with remote DNS resolution via the proxy (RFC 1928 §3).
    ///
    /// Preferred when the client should not leak DNS queries.
    Socks5h { host: String, port: u16 },
    /// HTTP CONNECT proxy (RFC 7231 §4.3.6).
    Http { host: String, port: u16 },
    /// HTTP CONNECT proxy over TLS.
    Https { host: String, port: u16 },
}

impl ProxyConfig {
    fn parts(&self) -> (&'static str, &str, u16) {
        match self {
            Self::Socks5 { host, port } => ("socks5", host, *port),
            Self::Socks5h { host, port } => ("socks5h", host, *port),
            Self::Http { host, port } => ("http", host, *port),
            Self::Https { host, port } => ("https", host, *port),
        }
    }

    /// Returns the proxy URL string for this configuration.
    ///
    /// The URL scheme determines which protocol `reqwest` uses to establish
    /// the tunnel.
    pub fn url(&self) -> Result<String> {
        let (scheme, host, port) = self.parts();
        let formatted_host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.to_owned()
        };
        let url = format!("{scheme}://{formatted_host}:{port}");
        reqwest::Url::parse(&url).with_context(|| format!("invalid proxy URL '{url}'"))?;
        Ok(url)
    }

    /// Build a `reqwest::Proxy` that forwards all traffic through this proxy.
    ///
    /// Uses `Proxy::all` so both HTTP and HTTPS requests are routed through
    /// the proxy — including the TLS handshake for HTTPS targets when using
    /// SOCKS5.
    pub fn into_reqwest_proxy(self) -> Result<Proxy> {
        let url = self.url()?;
        Proxy::all(&url).with_context(|| format!("invalid proxy URL '{url}'"))
    }

    /// Build a `reqwest::Client` pre-configured with this proxy.
    ///
    /// The returned client routes all requests through the proxy.  For
    /// SOCKS5 (RFC 1928) support the `reqwest` crate must be compiled with
    /// the `socks` feature (enabled in `Cargo.toml`).
    pub fn build_client(self) -> Result<reqwest::Client> {
        let proxy = self.into_reqwest_proxy()?;
        reqwest::Client::builder()
            .proxy(proxy)
            .build()
            .context("failed to build reqwest client with proxy")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socks5_url_format_rfc1928() {
        let cfg = ProxyConfig::Socks5 {
            host: "127.0.0.1".to_string(),
            port: 1080,
        };
        assert_eq!(
            cfg.url().expect("proxy url"),
            "socks5://127.0.0.1:1080",
            "RFC 1928 SOCKS5 URL scheme"
        );
    }

    #[test]
    fn socks5h_url_format_remote_dns_rfc1928() {
        let cfg = ProxyConfig::Socks5h {
            host: "proxy.example.com".to_string(),
            port: 1080,
        };
        assert_eq!(
            cfg.url().expect("proxy url"),
            "socks5h://proxy.example.com:1080",
            "socks5h for remote DNS resolution (RFC 1928 §3)"
        );
    }

    #[test]
    fn http_proxy_url_format() {
        let cfg = ProxyConfig::Http {
            host: "proxy.corp.example".to_string(),
            port: 3128,
        };
        assert_eq!(
            cfg.url().expect("proxy url"),
            "http://proxy.corp.example:3128"
        );
    }

    #[test]
    fn ipv6_proxy_url_format_uses_brackets() {
        let cfg = ProxyConfig::Socks5 {
            host: "2001:db8::1".to_string(),
            port: 1080,
        };
        assert_eq!(cfg.url().expect("proxy url"), "socks5://[2001:db8::1]:1080");
    }

    #[test]
    fn into_reqwest_proxy_socks5_succeeds_rfc1928() {
        let cfg = ProxyConfig::Socks5 {
            host: "127.0.0.1".to_string(),
            port: 1080,
        };
        // Smoke test: proxy construction must not error (no live connection needed).
        cfg.into_reqwest_proxy()
            .expect("SOCKS5 proxy construction (RFC 1928)");
    }

    #[test]
    fn build_client_with_socks5_proxy_rfc1928() {
        let cfg = ProxyConfig::Socks5 {
            host: "127.0.0.1".to_string(),
            port: 9050,
        };
        // Smoke test: client construction must not error (no live connection needed).
        cfg.build_client()
            .expect("reqwest client with SOCKS5 proxy (RFC 1928)");
    }
}
