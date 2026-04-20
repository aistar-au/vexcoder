//! JSON Web Token validation and generation.
//!
//! # Referenced Specifications
//!
//! | RFC | Title | Covered |
//! |-----|-------|---------|
//! | [RFC 7519](https://www.rfc-editor.org/rfc/rfc7519) | JSON Web Token (JWT) | Registered claims and compact serialisation for HS256 tokens |
//! | [RFC 7515](https://www.rfc-editor.org/rfc/rfc7515) | JSON Web Signature (JWS) | HS256 signing and verification only |
//!
//! The `typ` header, registered claim names (`iss`, `sub`, `aud`, `exp`,
//! `nbf`, `iat`, `jti`), and compact serialisation format follow RFC 7519.
//! Algorithm identifier (`HS256`) follows RFC 7518. RS256 and ES256 are out of
//! scope for this wrapper; callers that need them require an API extension,
//! not a wrapper tweak.

use anyhow::{Context, Result, anyhow};
use base64::Engine as _;
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, TokenData, Validation, decode, encode,
};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;

#[derive(Debug, Deserialize)]
struct RawJoseHeader {
    alg: Option<String>,
    typ: Option<String>,
    cty: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
enum AudienceClaim {
    One(String),
    Many(Vec<String>),
}

fn deserialize_optional_audience<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<AudienceClaim>::deserialize(deserializer)?;
    Ok(value.map(|audience| match audience {
        AudienceClaim::One(entry) => vec![entry],
        AudienceClaim::Many(entries) => entries,
    }))
}

/// Standard JWT registered claims (RFC 7519 §4.1).
///
/// All fields are optional as per the RFC — presence depends on the token
/// issuer's requirements.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisteredClaims {
    /// `iss` — Issuer (RFC 7519 §4.1.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    /// `sub` — Subject (RFC 7519 §4.1.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    /// `aud` — Audience (RFC 7519 §4.1.3).
    #[serde(
        default,
        deserialize_with = "deserialize_optional_audience",
        skip_serializing_if = "Option::is_none"
    )]
    pub aud: Option<Vec<String>>,
    /// `exp` — Expiration time (RFC 7519 §4.1.4), seconds since Unix epoch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<u64>,
    /// `nbf` — Not Before (RFC 7519 §4.1.5), seconds since Unix epoch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<u64>,
    /// `iat` — Issued At (RFC 7519 §4.1.6), seconds since Unix epoch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<u64>,
    /// `jti` — JWT ID (RFC 7519 §4.1.7), unique identifier for this token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
}

/// Encode a JWT with HMAC-SHA256 (HS256, RFC 7518 §3.2) using `secret`.
///
/// The `typ` header is set to `"JWT"` per RFC 7519 §5.1.
pub fn encode_hs256<C: Serialize>(claims: &C, secret: &[u8]) -> Result<String> {
    let mut header = Header::new(Algorithm::HS256);
    header.typ = Some("JWT".to_owned());
    encode(&header, claims, &EncodingKey::from_secret(secret))
        .context("JWT HS256 encoding failed (RFC 7519/7515)")
}

/// Decode and validate a JWT signed with HMAC-SHA256 (HS256).
///
/// This wrapper enforces RFC 7519 registered claims (`iss`, `aud`, `exp`) and
/// rejects RFC 8725 forbidden or unsupported header values such as `alg: none`,
/// missing `typ: JWT`, or unexpected `cty` values.
pub fn decode_hs256<C: for<'de> Deserialize<'de>>(
    token: &str,
    secret: &[u8],
    expected_issuer: &str,
    expected_audience: &str,
) -> Result<TokenData<C>> {
    let raw_header = peek_raw_header(token)?;
    if raw_header.alg.as_deref() == Some("none") {
        return Err(anyhow!(
            "JWT header uses forbidden alg=none (RFC 8725 §3.1)"
        ));
    }
    peek_header(token)?;
    if raw_header.typ.as_deref() != Some("JWT") {
        return Err(anyhow!(
            "JWT header typ must be JWT (RFC 7519 §5.1 / RFC 8725 §3.11)"
        ));
    }
    if raw_header.cty.is_some() {
        return Err(anyhow!(
            "nested JWT content types are not supported by this HS256 wrapper (RFC 8725 §3.11)"
        ));
    }

    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    validation.validate_aud = true;
    validation.required_spec_claims =
        HashSet::from(["exp".to_string(), "aud".to_string(), "iss".to_string()]);
    validation.set_audience(&[expected_audience]);
    validation.set_issuer(&[expected_issuer]);
    decode::<C>(token, &DecodingKey::from_secret(secret), &validation)
        .context("JWT HS256 validation failed (RFC 7519/7515)")
}

/// Validate the `exp` claim (RFC 7519 §4.1.4).
///
/// Returns an error if the claim is present and the token has expired.
/// Returns `Ok(())` if the claim is absent (expiry not enforced).
pub fn validate_expiry(claims: &RegisteredClaims, now_secs: u64) -> Result<()> {
    if let Some(exp) = claims.exp
        && now_secs >= exp
    {
        return Err(anyhow!(
            "JWT expired: exp={exp} now={now_secs} (RFC 7519 §4.1.4)"
        ));
    }
    Ok(())
}

/// Extract the unverified header from a JWT compact serialisation.
///
/// Per RFC 7519 §7.2 step 4, the JOSE header is decoded first to determine
/// the algorithm before signature verification.
pub fn peek_header(token: &str) -> Result<Header> {
    jsonwebtoken::decode_header(token).context("JWT header decode failed (RFC 7519 §7.2)")
}

fn peek_raw_header(token: &str) -> Result<RawJoseHeader> {
    let encoded = token
        .split('.')
        .next()
        .ok_or_else(|| anyhow!("JWT compact serialisation is missing a JOSE header segment"))?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .context("JWT JOSE header base64url decode failed")?;
    serde_json::from_slice(&decoded).context("JWT JOSE header JSON decode failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"super-secret-key-for-test-only";

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct TestClaims {
        #[serde(flatten)]
        registered: RegisteredClaims,
        custom: String,
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn hs256_encode_decode_roundtrip_rfc7519() {
        let claims = TestClaims {
            registered: RegisteredClaims {
                iss: Some("vexcoder".to_string()),
                sub: Some("test-subject".to_string()),
                aud: Some(vec!["cli".to_string()]),
                exp: Some(now() + 3600),
                nbf: None,
                iat: Some(now()),
                jti: Some("test-jti-001".to_string()),
            },
            custom: "hello".to_string(),
        };

        let token = encode_hs256(&claims, SECRET).expect("encode");
        assert!(!token.is_empty());
        // JWT compact serialisation: three base64url segments separated by '.'
        assert_eq!(
            token.split('.').count(),
            3,
            "RFC 7519 compact serialisation"
        );

        let decoded: TokenData<TestClaims> =
            decode_hs256(&token, SECRET, "vexcoder", "cli").expect("decode");
        assert_eq!(decoded.claims, claims);
        assert_eq!(decoded.header.alg, Algorithm::HS256);
    }

    #[test]
    fn wrong_secret_fails_validation_rfc7515() {
        let claims = RegisteredClaims {
            iss: Some("issuer".to_string()),
            sub: Some("s".to_string()),
            aud: Some(vec!["cli".to_string()]),
            exp: Some(now() + 60),
            nbf: None,
            iat: None,
            jti: None,
        };
        let token = encode_hs256(&claims, SECRET).expect("encode");
        let result = decode_hs256::<RegisteredClaims>(&token, b"wrong-secret", "issuer", "cli");
        assert!(
            result.is_err(),
            "mismatched HMAC must be rejected (RFC 7515)"
        );
    }

    #[test]
    fn single_string_audience_decodes_rfc7519() {
        #[derive(Serialize)]
        struct SingleAudienceClaims {
            iss: String,
            aud: String,
            exp: u64,
        }

        let token = encode_hs256(
            &SingleAudienceClaims {
                iss: "issuer".to_string(),
                aud: "cli".to_string(),
                exp: now() + 60,
            },
            SECRET,
        )
        .expect("encode");

        let decoded =
            decode_hs256::<RegisteredClaims>(&token, SECRET, "issuer", "cli").expect("decode");
        assert_eq!(decoded.claims.aud, Some(vec!["cli".to_string()]));
    }

    #[test]
    fn decode_hs256_rejects_missing_required_claims_rfc7519() {
        let claims = RegisteredClaims {
            iss: Some("issuer".to_string()),
            sub: None,
            aud: None,
            exp: None,
            nbf: None,
            iat: None,
            jti: None,
        };
        let token = encode_hs256(&claims, SECRET).expect("encode");
        let result = decode_hs256::<RegisteredClaims>(&token, SECRET, "issuer", "cli");
        assert!(result.is_err(), "missing aud/exp must be rejected");
    }

    #[test]
    fn decode_hs256_rejects_alg_none_rfc8725() {
        let header = serde_json::json!({"alg": "none", "typ": "JWT"});
        let claims = serde_json::json!({
            "iss": "issuer",
            "aud": "cli",
            "exp": now() + 60
        });
        let token = format!(
            "{}.{}.",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(&header).expect("header json")),
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(&claims).expect("claims json"))
        );

        let result = decode_hs256::<RegisteredClaims>(&token, SECRET, "issuer", "cli");
        assert!(result.is_err(), "alg=none must be rejected");
    }

    #[test]
    fn expired_token_fails_expiry_check_rfc7519() {
        let claims = RegisteredClaims {
            iss: None,
            sub: None,
            aud: None,
            exp: Some(now() - 1), // already expired
            nbf: None,
            iat: None,
            jti: None,
        };
        let result = validate_expiry(&claims, now());
        assert!(
            result.is_err(),
            "expired token must be rejected (RFC 7519 §4.1.4)"
        );
    }

    #[test]
    fn token_at_expiry_is_rejected_rfc7519() {
        let instant = now();
        let claims = RegisteredClaims {
            iss: None,
            sub: None,
            aud: None,
            exp: Some(instant),
            nbf: None,
            iat: None,
            jti: None,
        };
        let result = validate_expiry(&claims, instant);
        assert!(
            result.is_err(),
            "exp must be treated as expired at the exact timestamp"
        );
    }

    #[test]
    fn no_exp_claim_passes_validation() {
        let claims = RegisteredClaims {
            iss: None,
            sub: None,
            aud: None,
            exp: None,
            nbf: None,
            iat: None,
            jti: None,
        };
        validate_expiry(&claims, now()).expect("no exp claim should not error");
    }

    #[test]
    fn peek_header_reads_algorithm_rfc7519() {
        let claims = RegisteredClaims {
            iss: None,
            sub: None,
            aud: None,
            exp: None,
            nbf: None,
            iat: None,
            jti: None,
        };
        let token = encode_hs256(&claims, SECRET).expect("encode");
        let header = peek_header(&token).expect("peek header");
        assert_eq!(header.alg, Algorithm::HS256);
        assert_eq!(header.typ.as_deref(), Some("JWT"));
    }
}
