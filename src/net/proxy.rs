use anyhow::{Context, Result};
use reqwest::Proxy;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyConfig {
    Socks5 { host: String, port: u16 },

    Socks5h { host: String, port: u16 },

    Http { host: String, port: u16 },

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

    pub fn into_reqwest_proxy(self) -> Result<Proxy> {
        let url = self.url()?;
        Proxy::all(&url).with_context(|| format!("invalid proxy URL '{url}'"))
    }

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

        cfg.into_reqwest_proxy()
            .expect("SOCKS5 proxy construction (RFC 1928)");
    }

    #[test]
    fn build_client_with_socks5_proxy_rfc1928() {
        let cfg = ProxyConfig::Socks5 {
            host: "127.0.0.1".to_string(),
            port: 9050,
        };

        cfg.build_client()
            .expect("reqwest client with SOCKS5 proxy (RFC 1928)");
    }
}
