//! OAuth 2.0 PKCE flow and Bearer token utilities.
//!
//! # RFC compliance
//!
//! | RFC | Title | Covered |
//! |-----|-------|---------|
//! | [RFC 7636](https://www.rfc-editor.org/rfc/rfc7636) | PKCE for OAuth Public Clients | `PkceChallenge`, `generate_pkce_pair` |
//! | [RFC 6750](https://www.rfc-editor.org/rfc/rfc6750) | OAuth 2.0 Bearer Token Usage | `bearer_header_value` |
//! | [RFC 6749](https://www.rfc-editor.org/rfc/rfc6749) | OAuth 2.0 Authorization Framework | `AuthorizationRequest` |
//!
//! The PKCE challenge method `S256` (SHA-256) is the only supported method per
//! RFC 7636 §4.2 ("If the client is capable of using "S256", it MUST use
//! "S256"").  The `plain` method is intentionally not implemented.

use anyhow::{Context, Result};
use oauth2::{
    AuthUrl, ClientId, CsrfToken, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenUrl,
};

/// A PKCE code verifier/challenge pair (RFC 7636 §4.1 and §4.2).
pub struct PkceChallenge {
    /// The code verifier — kept secret, sent in the token request.
    pub verifier: PkceCodeVerifier,
    /// The code challenge — sent in the authorization request.
    pub challenge: PkceCodeChallenge,
}

/// Generate a cryptographically random PKCE verifier and the corresponding
/// S256 challenge (RFC 7636 §4.1).
///
/// The verifier is 32 random bytes base64url-encoded (256 bits of entropy),
/// satisfying RFC 7636 §4.1 length requirements (43–128 characters after
/// encoding).
pub fn generate_pkce_pair() -> PkceChallenge {
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    PkceChallenge {
        verifier,
        challenge,
    }
}

/// Parameters for building an OAuth 2.0 authorization URL (RFC 6749 §4.1.1).
pub struct AuthorizationRequest {
    pub client_id: String,
    pub auth_url: String,
    pub redirect_url: String,
    pub scopes: Vec<String>,
}

/// Build an authorization URL with PKCE (RFC 6749 §4.1.1 + RFC 7636 §4.3).
///
/// Returns `(authorization_url, csrf_token, pkce_verifier)`.
/// The caller must:
/// 1. Redirect the user to `authorization_url`.
/// 2. Store `csrf_token` and `pkce_verifier` in session state.
/// 3. Exchange the returned authorization code using `pkce_verifier`.
pub fn build_authorization_url(
    req: &AuthorizationRequest,
) -> Result<(String, CsrfToken, PkceCodeVerifier)> {
    let client = oauth2::basic::BasicClient::new(
        ClientId::new(req.client_id.clone()),
        None,
        AuthUrl::new(req.auth_url.clone()).context("invalid auth_url (RFC 6749 §3.1)")?,
        None::<TokenUrl>,
    )
    .set_redirect_uri(
        RedirectUrl::new(req.redirect_url.clone())
            .context("invalid redirect_url (RFC 6749 §3.1.2)")?,
    );

    let pkce = generate_pkce_pair();
    let mut builder = client
        .authorize_url(CsrfToken::new_random)
        .set_pkce_challenge(pkce.challenge);

    for scope in &req.scopes {
        builder = builder.add_scope(Scope::new(scope.clone()));
    }

    let (url, csrf) = builder.url();
    Ok((url.to_string(), csrf, pkce.verifier))
}

/// Format a Bearer token for use in the `Authorization` header (RFC 6750 §2.1).
///
/// Returns the header value string `"Bearer <token>"`.
pub fn bearer_header_value(token: &str) -> String {
    format!("Bearer {token}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_verifier_and_challenge_are_distinct_rfc7636() {
        let pair = generate_pkce_pair();
        // The verifier and challenge strings must differ (S256 applies SHA-256+base64url).
        let verifier_str = pair.verifier.secret();
        let challenge_str = pair.challenge.as_str();
        assert_ne!(
            verifier_str, challenge_str,
            "verifier and S256 challenge must differ (RFC 7636 §4.2)"
        );
    }

    #[test]
    fn pkce_verifier_meets_length_requirements_rfc7636() {
        let pair = generate_pkce_pair();
        let len = pair.verifier.secret().len();
        // RFC 7636 §4.1: code_verifier length 43–128 characters.
        assert!(
            (43..=128).contains(&len),
            "verifier length {len} out of RFC 7636 §4.1 range 43–128"
        );
    }

    #[test]
    fn pkce_challenge_method_is_s256_rfc7636() {
        let pair = generate_pkce_pair();
        assert_eq!(
            pair.challenge.method().as_str(),
            "S256",
            "only S256 is supported (RFC 7636 §4.2)"
        );
    }

    #[test]
    fn bearer_header_value_rfc6750() {
        let hdr = bearer_header_value("my-access-token");
        assert_eq!(hdr, "Bearer my-access-token", "RFC 6750 §2.1 format");
        assert!(hdr.starts_with("Bearer "), "RFC 6750 §2.1 prefix");
    }

    #[test]
    fn build_authorization_url_contains_pkce_and_scope_rfc6749_rfc7636() {
        let req = AuthorizationRequest {
            client_id: "test-client".to_string(),
            auth_url: "https://auth.example.com/authorize".to_string(),
            redirect_url: "https://app.example.com/callback".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
        };
        let (url_str, _csrf, _verifier) = build_authorization_url(&req).expect("build auth url");
        // RFC 7636 §4.3: code_challenge and code_challenge_method must be present.
        assert!(
            url_str.contains("code_challenge="),
            "RFC 7636 §4.3 code_challenge"
        );
        assert!(
            url_str.contains("code_challenge_method=S256"),
            "RFC 7636 §4.3 S256 method"
        );
        // RFC 6749 §4.1.1: response_type, client_id, redirect_uri, scope.
        assert!(
            url_str.contains("response_type=code"),
            "RFC 6749 §4.1.1 response_type"
        );
        assert!(
            url_str.contains("client_id=test-client"),
            "RFC 6749 §4.1.1 client_id"
        );
        assert!(url_str.contains("scope="), "RFC 6749 §4.1.1 scope");
    }
}
