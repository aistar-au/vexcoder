use anyhow::{Context, Result, anyhow, bail};
use axum::Json;
use axum::http::StatusCode;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::io::BufReader;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::sync::Arc;

#[cfg(unix)]
use super::ResolvedUnixSurface;
use super::{ControlResponse, HttpSurfaceSettings, ResolvedHttpSurface, ResolvedServeConfig};
use crate::config::Config;

pub fn resolve_serve_config(
    config: &Config,
    host_override: Option<String>,
    port_override: Option<u16>,
) -> Result<ResolvedServeConfig> {
    if config.api.tls_skip_verify {
        bail!("api.tls_skip_verify must remain false in Phase I");
    }
    if config.api.vpn_trust {
        bail!("api.vpn_trust must remain false until a dedicated ADR exists");
    }

    let mut resolved = ResolvedServeConfig {
        http: None,
        unix: None,
    };
    let transport = config.api.transport;

    if transport.http_enabled() {
        let bind_addr = host_override.unwrap_or_else(|| config.api.host.clone());
        let port = port_override.unwrap_or(config.api.port);
        let bearer_token = config
            .api
            .key
            .clone()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                anyhow!("LocalApiServer HTTP transport requires VEX_API_KEY or api.key")
            })?;
        let is_loopback = is_strict_loopback_host(&bind_addr, port)?;
        let tls = build_http_tls_config(config, is_loopback)?;
        resolved.http = Some(ResolvedHttpSurface {
            bind_addr,
            port,
            auth: HttpSurfaceSettings {
                bearer_token: Arc::<str>::from(bearer_token),
                hsts_enabled: tls.is_some(),
            },
            tls,
        });
    }

    if transport.unix_enabled() {
        #[cfg(unix)]
        {
            resolved.unix = Some(ResolvedUnixSurface {
                socket_path: config
                    .api
                    .socket
                    .clone()
                    .unwrap_or_else(default_unix_socket_path),
            });
        }
        #[cfg(not(unix))]
        {
            bail!("Unix-socket transport is only supported on macOS and Linux");
        }
    }

    Ok(resolved)
}

pub fn is_strict_loopback_host(host: &str, port: u16) -> Result<bool> {
    let normalized = host.trim().to_ascii_lowercase();
    if let Ok(addr) = normalized.parse::<std::net::IpAddr>() {
        return Ok(addr.is_loopback());
    }
    if normalized != "localhost" {
        return Ok(false);
    }

    let addrs = (host, port)
        .to_socket_addrs()
        .with_context(|| format!("failed to resolve LocalApiServer host '{host}'"))?
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        bail!("failed to resolve LocalApiServer host '{host}'");
    }
    Ok(addrs.iter().all(|addr| addr.ip().is_loopback()))
}

pub fn build_http_tls_config(
    config: &Config,
    is_loopback: bool,
) -> Result<Option<Arc<rustls::ServerConfig>>> {
    if let Some(ca_path) = config.api.tls_ca_cert.as_deref() {
        validate_pem_certificates(ca_path)
            .with_context(|| format!("invalid api.tls_ca_cert '{}'", ca_path.display()))?;
    }

    let cert_path = config.api.tls_cert.as_deref();
    let key_path = config.api.tls_key.as_deref();
    let tls_requested = cert_path.is_some() || key_path.is_some();

    if !is_loopback && !tls_requested {
        bail!("LocalApiServer non-loopback HTTP bind requires both api.tls_cert and api.tls_key");
    }
    if !tls_requested {
        return Ok(None);
    }

    let cert_path =
        cert_path.ok_or_else(|| anyhow!("api.tls_cert is required when TLS is enabled"))?;
    let key_path =
        key_path.ok_or_else(|| anyhow!("api.tls_key is required when TLS is enabled"))?;
    let cert_chain = load_pem_certificates(cert_path)
        .with_context(|| format!("invalid api.tls_cert '{}'", cert_path.display()))?;
    let private_key = load_private_key(key_path)
        .with_context(|| format!("invalid api.tls_key '{}'", key_path.display()))?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut tls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)
        .context(
            "api.tls_cert and api.tls_key must form a matching certificate/private-key pair",
        )?;
    tls_config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Some(Arc::new(tls_config)))
}

fn load_pem_certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to read PEM certificates from '{}'", path.display()))?;
    let mut reader = BufReader::new(file);
    let certificates = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to parse PEM certificates from '{}'", path.display()))?;
    if certificates.is_empty() {
        bail!("no PEM certificates found in '{}'", path.display());
    }
    Ok(certificates)
}

fn validate_pem_certificates(path: &Path) -> Result<()> {
    let _ = load_pem_certificates(path)?;
    Ok(())
}

fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let pkcs8 = load_first_private_key(path, PrivateKeyFormat::Pkcs8)?;
    if let Some(key) = pkcs8 {
        return Ok(key);
    }

    let sec1 = load_first_private_key(path, PrivateKeyFormat::Sec1)?;
    if let Some(key) = sec1 {
        return Ok(key);
    }

    let rsa = load_first_private_key(path, PrivateKeyFormat::Rsa)?;
    if let Some(key) = rsa {
        return Ok(key);
    }

    bail!("no supported PEM private key found in '{}'", path.display())
}

enum PrivateKeyFormat {
    Pkcs8,
    Sec1,
    Rsa,
}

fn load_first_private_key(
    path: &Path,
    format: PrivateKeyFormat,
) -> Result<Option<PrivateKeyDer<'static>>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to read PEM private key from '{}'", path.display()))?;
    let mut reader = BufReader::new(file);
    let key = match format {
        PrivateKeyFormat::Pkcs8 => rustls_pemfile::pkcs8_private_keys(&mut reader)
            .next()
            .transpose()?
            .map(Into::into),
        PrivateKeyFormat::Sec1 => rustls_pemfile::ec_private_keys(&mut reader)
            .next()
            .transpose()?
            .map(Into::into),
        PrivateKeyFormat::Rsa => rustls_pemfile::rsa_private_keys(&mut reader)
            .next()
            .transpose()?
            .map(Into::into),
    };
    Ok(key)
}

#[cfg(unix)]
pub fn default_unix_socket_path() -> std::path::PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("vexcoder.sock")
}

pub fn bad_request(reason: &'static str) -> (StatusCode, Json<ControlResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ControlResponse {
            ok: false,
            reason: Some(reason),
        }),
    )
}

pub fn not_found(reason: &'static str) -> (StatusCode, Json<ControlResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(ControlResponse {
            ok: false,
            reason: Some(reason),
        }),
    )
}

pub fn conflict(reason: &'static str) -> (StatusCode, Json<ControlResponse>) {
    (
        StatusCode::CONFLICT,
        Json(ControlResponse {
            ok: false,
            reason: Some(reason),
        }),
    )
}

pub fn internal_error(_: serde_json::Error) -> (StatusCode, Json<ControlResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ControlResponse {
            ok: false,
            reason: Some("internal_error"),
        }),
    )
}
