use anyhow::{Context, Result};
use hickory_resolver::{
    TokioResolver,
    config::{NameServerConfig, ResolverConfig, ResolverOpts},
    net::runtime::TokioRuntimeProvider,
};
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

pub mod doh_endpoints {

    pub const CLOUDFLARE: &str = "https://cloudflare-dns.com/dns-query";
}

pub struct DohResolver {
    inner: TokioResolver,
}

impl DohResolver {
    pub fn new(doh_server_ip: IpAddr, doh_server_port: u16, tls_dns_name: &str) -> Result<Self> {
        let mut name_server = NameServerConfig::https(doh_server_ip, Arc::from(tls_dns_name), None);
        name_server.trust_negative_responses = false;
        if let Some(connection) = name_server.connections.first_mut() {
            connection.port = doh_server_port;
        }

        let mut config = ResolverConfig::default();
        config.add_name_server(name_server);

        let mut opts = ResolverOpts::default();

        opts.ip_strategy = hickory_resolver::config::LookupIpStrategy::Ipv6thenIpv4;
        opts.timeout = Duration::from_secs(5);
        opts.attempts = 2;

        let mut builder =
            TokioResolver::builder_with_config(config, TokioRuntimeProvider::default());
        *builder.options_mut() = opts;
        let inner = builder.build().context("failed to build DoH resolver")?;
        Ok(Self { inner })
    }

    pub fn cloudflare() -> Result<Self> {
        Self::new(
            IpAddr::from_str("1.1.1.1").unwrap(),
            443,
            "cloudflare-dns.com",
        )
    }

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
            IpAddr::from_str("93.184.216.34").unwrap(),
            IpAddr::from_str("2001:db8::1").unwrap(),
            IpAddr::from_str("192.0.2.1").unwrap(),
            IpAddr::from_str("2001:db8::2").unwrap(),
        ];
        happy_sort(&mut addrs);

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
        DohResolver::cloudflare().expect("cloudflare DoH resolver construction");
    }
}
