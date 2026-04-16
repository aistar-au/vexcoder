//! HTTP body compression/decompression.
//!
//! # RFC compliance
//!
//! | RFC | Title | Covered |
//! |-----|-------|---------|
//! | [RFC 1952](https://www.rfc-editor.org/rfc/rfc1952) | GZIP file format | `Encoding::Gzip` |
//! | [RFC 7932](https://www.rfc-editor.org/rfc/rfc7932) | Brotli compressed data | `Encoding::Brotli` |
//!
//! `Content-Encoding` and `Transfer-Encoding` token names follow
//! [RFC 7231 §3.1.2.2](https://www.rfc-editor.org/rfc/rfc7231#section-3.1.2.2) and
//! [RFC 7230 §4](https://www.rfc-editor.org/rfc/rfc7230#section-4).

use anyhow::{Context, Result};
use async_compression::tokio::bufread::{BrotliDecoder, GzipDecoder, ZlibDecoder};
use bytes::Bytes;
use std::fmt;
use tokio::io::AsyncReadExt;
use tokio_util::io::StreamReader;

/// Content-Encoding values defined by HTTP/1.1 and extensions.
///
/// Variants map directly to the token strings in `Content-Encoding` headers
/// as registered in the
/// [HTTP Content Coding Registry](https://www.iana.org/assignments/http-parameters/http-parameters.xhtml#content-coding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// `identity` — no transformation (RFC 7231 §3.1.2.2).
    Identity,
    /// `gzip` — GZIP compression (RFC 1952).
    Gzip,
    /// `br` — Brotli compression (RFC 7932).
    Brotli,
    /// `deflate` — zlib-wrapped DEFLATE (RFC 1950 / RFC 7230).
    Deflate,
}

impl Encoding {
    /// Parse the token string from a `Content-Encoding` or `Accept-Encoding` header.
    pub fn from_token(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "identity" => Some(Self::Identity),
            "gzip" | "x-gzip" => Some(Self::Gzip),
            "br" => Some(Self::Brotli),
            "deflate" => Some(Self::Deflate),
            _ => None,
        }
    }

    /// Returns the canonical token string for use in HTTP headers.
    pub fn as_token(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Gzip => "gzip",
            Self::Brotli => "br",
            Self::Deflate => "deflate",
        }
    }
}

impl fmt::Display for Encoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_token())
    }
}

/// Returns the `Accept-Encoding` header value advertising all supported
/// encodings in preference order: `br, gzip, deflate, identity`.
pub fn accept_encoding_header() -> &'static str {
    "br, gzip, deflate, identity"
}

/// Decompress `bytes` according to `encoding`.
///
/// Returns the original bytes unchanged for `Encoding::Identity`.
/// Returns an error for unknown or unsupported encodings.
pub async fn decompress(bytes: Bytes, encoding: Encoding) -> Result<Bytes> {
    match encoding {
        Encoding::Identity => Ok(bytes),
        Encoding::Gzip => {
            let stream = futures::stream::once(async move { Ok::<_, std::io::Error>(bytes) });
            let reader = StreamReader::new(Box::pin(stream));
            let mut decoder = GzipDecoder::new(reader);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .await
                .context("gzip decompression failed (RFC 1952)")?;
            Ok(Bytes::from(out))
        }
        Encoding::Brotli => {
            let stream = futures::stream::once(async move { Ok::<_, std::io::Error>(bytes) });
            let reader = StreamReader::new(Box::pin(stream));
            let mut decoder = BrotliDecoder::new(reader);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .await
                .context("brotli decompression failed (RFC 7932)")?;
            Ok(Bytes::from(out))
        }
        Encoding::Deflate => {
            let stream = futures::stream::once(async move { Ok::<_, std::io::Error>(bytes) });
            let reader = StreamReader::new(Box::pin(stream));
            let mut decoder = ZlibDecoder::new(reader);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .await
                .context("deflate decompression failed (RFC 1950)")?;
            Ok(Bytes::from(out))
        }
    }
}

/// Compress `bytes` using GZIP (RFC 1952).
pub async fn compress_gzip(bytes: Bytes) -> Result<Bytes> {
    use async_compression::tokio::write::GzipEncoder;
    use tokio::io::AsyncWriteExt;

    let mut encoder = GzipEncoder::new(Vec::new());
    encoder
        .write_all(&bytes)
        .await
        .context("gzip compression write failed")?;
    encoder.shutdown().await.context("gzip compression flush failed")?;
    Ok(Bytes::from(encoder.into_inner()))
}

/// Compress `bytes` using Brotli (RFC 7932).
pub async fn compress_brotli(bytes: Bytes) -> Result<Bytes> {
    use async_compression::tokio::write::BrotliEncoder;
    use tokio::io::AsyncWriteExt;

    let mut encoder = BrotliEncoder::new(Vec::new());
    encoder
        .write_all(&bytes)
        .await
        .context("brotli compression write failed")?;
    encoder.shutdown().await.context("brotli compression flush failed")?;
    Ok(Bytes::from(encoder.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip_gzip_rfc1952() {
        let original = b"hello, gzip RFC 1952 world!".as_slice();
        let compressed = compress_gzip(Bytes::from_static(original)).await.unwrap();
        assert_ne!(compressed, Bytes::from_static(original));
        let decompressed = decompress(compressed, Encoding::Gzip).await.unwrap();
        assert_eq!(decompressed, Bytes::from_static(original));
    }

    #[tokio::test]
    async fn roundtrip_brotli_rfc7932() {
        let original = b"hello, brotli RFC 7932 world!".as_slice();
        let compressed = compress_brotli(Bytes::from_static(original)).await.unwrap();
        assert_ne!(compressed, Bytes::from_static(original));
        let decompressed = decompress(compressed, Encoding::Brotli).await.unwrap();
        assert_eq!(decompressed, Bytes::from_static(original));
    }

    #[tokio::test]
    async fn identity_encoding_is_passthrough() {
        let data = Bytes::from_static(b"passthrough");
        let result = decompress(data.clone(), Encoding::Identity).await.unwrap();
        assert_eq!(result, data);
    }

    #[test]
    fn encoding_token_roundtrip() {
        for (token, expected) in &[
            ("gzip", Encoding::Gzip),
            ("br", Encoding::Brotli),
            ("deflate", Encoding::Deflate),
            ("identity", Encoding::Identity),
            ("x-gzip", Encoding::Gzip),
        ] {
            assert_eq!(Encoding::from_token(token), Some(*expected));
            assert_eq!(expected.as_token(), expected.as_token()); // reflexive
        }
        assert_eq!(Encoding::from_token("unknown"), None);
    }

    #[test]
    fn accept_encoding_header_contains_all_schemes() {
        let hdr = accept_encoding_header();
        assert!(hdr.contains("br"));
        assert!(hdr.contains("gzip"));
        assert!(hdr.contains("deflate"));
    }
}
