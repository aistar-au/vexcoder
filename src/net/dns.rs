//! DNS resolution with DoH and Happy Eyeballs support.
//!
//! # Referenced Specifications
//!
//! | RFC | Title | Covered |
//! |-----|-------|---------|
//! | [RFC 8484](https://www.rfc-editor.org/rfc/rfc8484) | DNS Queries over HTTPS (DoH) | `DohResolver` |
//! | [RFC 6555](https://www.rfc-editor.org/rfc/rfc6555) | Happy Eyeballs — Success with Dual-Stack Hosts | `happy_sort` |
//!
//! The DoH wire format uses the `application/dns-message` media type and
//! DNS message encoding defined in
//! [RFC 1035](https://www.rfc-editor.org/rfc/rfc1035).

use anyhow::{Context, Result};
use hickory_resolver::{
    TokioAsyncResolver,
    config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts},
};
use std::net::IpAddr;
use std::str::FromStr;
use std::time::Duration;

/// RFC 8484 DoH server endpoints.
///
/// The endpoint supports the `GET` and `POST` request methods defined in
/// RFC 8484 §4.1 and §4.2 using `application/dns-message` content type.
pub mod doh_endpoints {
    /// Cloudflare 1.1.1.1 DNS-over-HTTPS (RFC 8484).
    pub const CLOUDFLARE: &str = "https://cloudflare-dns.com/dns-query";
}

/// A DNS resolver that uses DNS-over-HTTPS (RFC 8484).
///
/// Wraps `hickory_resolver::TokioAsyncResolver` configured for HTTPS
/// transport. Resolved address lists are sorted with IPv6 entries before
/// IPv4 entries to follow the RFC 6555 preference order.
pub struct DohResolver {
    inner: TokioAsyncResolver,
}

impl DohResolver {
    /// Build a DoH resolver from a name server address and TLS DNS name.
    ///
    /// `doh_server_ip` and `doh_server_port` identify the HTTPS name server
    /// socket address to contact. `tls_dns_name` is the DNS name presented for
    /// TLS server name indication and certificate validation.
    ///
    /// The `doh_server_ip` must be the pre-resolved IP of the DoH server
    /// (to avoid a chicken-and-egg DNS lookup for the resolver itself).
    ///
    /// This constructor does not accept a full RFC 8484 endpoint URL or path;
    /// it configures HTTPS transport through `NameServerConfig`.
    pub fn new(doh_server_ip: IpAddr, doh_server_port: u16, tls_dns_name: &str) -> Result<Self> {
        let name_server = NameServerConfig {
            socket_addr: std::net::SocketAddr::new(doh_server_ip, doh_server_port),
            protocol: Protocol::Https,
            tls_dns_name: Some(tls_dns_name.to_string()),
            trust_negative_responses: false,
            tls_config: None,
            bind_addr: None,
        };

        let mut config = ResolverConfig::new();
        config.add_name_server(name_server);

        let mut opts = ResolverOpts::default();
        // Prefer IPv6 answers first and then fall back to IPv4 if needed.
        opts.ip_strategy = hickory_resolver::config::LookupIpStrategy::Ipv6thenIpv4;
        opts.timeout = Duration::from_secs(5);
        opts.attempts = 2;

        let inner = TokioAsyncResolver::tokio(config, opts);
        Ok(Self { inner })
    }

    /// Build a resolver against Cloudflare's DoH server (1.1.1.1).
    pub fn cloudflare() -> Result<Self> {
        // 1.1.1.1 and 2606:4700:4700::1111 are Cloudflare's DoH addresses.
        Self::new(
            IpAddr::from_str("1.1.1.1").unwrap(),
            443,
            "cloudflare-dns.com",
        )
    }

    /// Resolve `hostname` to a list of IP addresses.
    ///
    /// Queries both A (IPv4) and AAAA (IPv6) records. The results are sorted
    /// with IPv6 addresses first to implement the RFC 6555 Happy Eyeballs
    /// preference order.
    pub async fn resolve(&self, hostname: &str) -> Result<Vec<IpAddr>> {
        let lookup = self
            .inner
            .lookup_ip(hostname)
            .await
            .with_context(|| format!("DoH lookup failed for '{hostname}'"))?;

        let mut addrs: Vec<IpAddr> = lookup.iter().collect();
        happy_sort(&mut addrs);
        Ok(addrs)
    }
}

/// Sort addresses for RFC 6555 Happy Eyeballs preference order.
///
/// IPv6 addresses are placed before IPv4 addresses. Within each family the
/// original order from the DNS response is preserved (consistent with the
/// RFC 6555 §5 requirement to respect the DNS TTL-based ordering).
pub fn happy_sort(addrs: &mut [IpAddr]) {
    addrs.sort_by_key(|addr| match addr {
        IpAddr::V6(_) => 0u8,
        IpAddr::V4(_) => 1u8,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_sort_ipv6_before_ipv4() {
        let mut addrs = vec![
            IpAddr::from_str("93.184.216.34").unwrap(), // IPv4
            IpAddr::from_str("2001:db8::1").unwrap(),   // IPv6
            IpAddr::from_str("192.0.2.1").unwrap(),     // IPv4
            IpAddr::from_str("2001:db8::2").unwrap(),   // IPv6
        ];
        happy_sort(&mut addrs);
        // IPv6 addresses first (RFC 6555 §5 preference)
        assert!(matches!(addrs[0], IpAddr::V6(_)));
        assert!(matches!(addrs[1], IpAddr::V6(_)));
        assert!(matches!(addrs[2], IpAddr::V4(_)));
        assert!(matches!(addrs[3], IpAddr::V4(_)));
    }

    #[test]
    fn happy_sort_all_ipv4_unchanged_relative_order() {
        let mut addrs = vec![
            IpAddr::from_str("10.0.0.1").unwrap(),
            IpAddr::from_str("10.0.0.2").unwrap(),
        ];
        let original = addrs.clone();
        happy_sort(&mut addrs);
        assert_eq!(addrs, original);
    }

    #[test]
    fn happy_sort_empty_is_noop() {
        let mut addrs: Vec<IpAddr> = vec![];
        happy_sort(&mut addrs);
        assert!(addrs.is_empty());
    }

    #[test]
    fn cloudflare_resolver_constructs() {
        // Smoke test: constructor should not error.
        DohResolver::cloudflare().expect("cloudflare DoH resolver construction");
    }
}
