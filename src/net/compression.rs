//! HTTP body compression/decompression.
//!
//! # Referenced Specifications
//!
//! | RFC | Title | Covered |
//! |-----|-------|---------|
//! | [RFC 1952](https://www.rfc-editor.org/rfc/rfc1952) | GZIP file format | `Encoding::Gzip` |
//! | [RFC 7932](https://www.rfc-editor.org/rfc/rfc7932) | Brotli compressed data | `Encoding::Brotli` |
//!
//! `Content-Encoding` and `Transfer-Encoding` token names follow
//! [RFC 7231 §3.1.2.2](https://www.rfc-editor.org/rfc/rfc7231#section-3.1.2.2) and
//! [RFC 7230 §4](https://www.rfc-editor.org/rfc/rfc7230#section-4).

use anyhow::{Context, Result, anyhow};
use async_compression::tokio::bufread::{BrotliDecoder, GzipDecoder, ZlibDecoder};
use bytes::Bytes;
use std::fmt;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::io::StreamReader;

const DEFAULT_MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

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

    /// Returns the normalized token string for use in HTTP headers.
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
/// The decompressed payload is capped at 8 MiB to avoid unbounded memory
/// growth when handling untrusted bodies.
pub async fn decompress(bytes: Bytes, encoding: Encoding) -> Result<Bytes> {
    decompress_with_limit(bytes, encoding, DEFAULT_MAX_OUTPUT_BYTES).await
}

/// Decompress `bytes` according to `encoding` with an explicit output cap.
///
/// Returns an error if decompression fails or if the decompressed payload
/// exceeds `max_output_bytes`.
pub async fn decompress_with_limit(
    bytes: Bytes,
    encoding: Encoding,
    max_output_bytes: usize,
) -> Result<Bytes> {
    match encoding {
        Encoding::Identity => Ok(bytes),
        Encoding::Gzip => {
            let stream = futures::stream::once(async move { Ok::<_, std::io::Error>(bytes) });
            let reader = StreamReader::new(Box::pin(stream));
            decompress_reader_with_limit(
                GzipDecoder::new(reader),
                max_output_bytes,
                "gzip",
                "RFC 1952",
            )
            .await
        }
        Encoding::Brotli => {
            let stream = futures::stream::once(async move { Ok::<_, std::io::Error>(bytes) });
            let reader = StreamReader::new(Box::pin(stream));
            decompress_reader_with_limit(
                BrotliDecoder::new(reader),
                max_output_bytes,
                "brotli",
                "RFC 7932",
            )
            .await
        }
        Encoding::Deflate => {
            let stream = futures::stream::once(async move { Ok::<_, std::io::Error>(bytes) });
            let reader = StreamReader::new(Box::pin(stream));
            decompress_reader_with_limit(
                ZlibDecoder::new(reader),
                max_output_bytes,
                "deflate",
                "RFC 1950",
            )
            .await
        }
    }
}

async fn decompress_reader_with_limit<R>(
    decoder: R,
    max_output_bytes: usize,
    encoding_name: &str,
    rfc: &str,
) -> Result<Bytes>
where
    R: AsyncRead + Unpin,
{
    let read_limit = u64::try_from(max_output_bytes)
        .context("decompression limit exceeds supported reader size")?
        .checked_add(1)
        .context("decompression limit is too large")?;
    let mut limited = decoder.take(read_limit);
    let mut out = Vec::new();
    limited
        .read_to_end(&mut out)
        .await
        .with_context(|| format!("{encoding_name} decompression failed ({rfc})"))?;
    if out.len() > max_output_bytes {
        return Err(anyhow!(
            "{encoding_name} decompression exceeded {max_output_bytes} bytes"
        ));
    }
    Ok(Bytes::from(out))
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
    encoder
        .shutdown()
        .await
        .context("gzip compression flush failed")?;
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
    encoder
        .shutdown()
        .await
        .context("brotli compression flush failed")?;
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
            assert_eq!(Encoding::from_token(expected.as_token()), Some(*expected));
        }
        assert_eq!(Encoding::from_token("unknown"), None);
    }

    #[tokio::test]
    async fn decompress_limit_rejects_expansion() {
        let original = Bytes::from(vec![b'a'; 1024]);
        let compressed = compress_gzip(original).await.expect("compress gzip");
        let result = decompress_with_limit(compressed, Encoding::Gzip, 64).await;
        assert!(
            result.is_err(),
            "decompression cap must reject oversized output"
        );
    }

    #[test]
    fn accept_encoding_header_contains_all_schemes() {
        let hdr = accept_encoding_header();
        assert!(hdr.contains("br"));
        assert!(hdr.contains("gzip"));
        assert!(hdr.contains("deflate"));
    }
}
