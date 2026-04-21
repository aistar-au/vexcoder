//! OAuth 2.0 PKCE flow and Bearer token utilities.
//!
//! # Referenced Specifications
//!
//! | RFC | Title | Covered |
//! |-----|-------|---------|
//! | [RFC 8252](https://www.rfc-editor.org/rfc/rfc8252) | OAuth 2.0 for Native Apps | `NativeAuthorizationRequest`, `build_native_authorization_url`, `open_authorization_url`, `exchange_native_authorization_code` |
//! | [RFC 7636](https://www.rfc-editor.org/rfc/rfc7636) | PKCE for OAuth Public Clients | `PkceChallenge`, `generate_pkce_pair` |
//! | [RFC 6750](https://www.rfc-editor.org/rfc/rfc6750) | OAuth 2.0 Bearer Token Usage | `bearer_header_value` |
//! | [RFC 6749](https://www.rfc-editor.org/rfc/rfc6749) | OAuth 2.0 Authorization Framework | `AuthorizationRequest` |
//!
//! The PKCE challenge method `S256` (SHA-256) is the only supported method per
//! RFC 7636 §4.2 ("If the client is capable of using "S256", it MUST use
//! "S256"").  The `plain` method is intentionally not implemented.

use anyhow::{Context, Result, bail};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, HttpRequest, HttpResponse, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use std::time::Duration;
use thiserror::Error;

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

/// Parameters for an OAuth 2.0 native-app authorization request
/// (RFC 8252 §7.3 + RFC 6749 §4.1.1).
pub struct NativeAuthorizationRequest {
    pub client_id: String,
    pub auth_url: String,
    pub redirect_url: String,
    pub scopes: Vec<String>,
}

/// Parameters for exchanging an authorization code from a native-app PKCE flow
/// (RFC 8252 §8.1 + RFC 6749 §4.1.3).
pub struct NativeCodeExchangeRequest {
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
    pub redirect_url: String,
    pub authorization_code: String,
    pub pkce_verifier: String,
}

/// Normalized token response for a native-app authorization code exchange.
pub struct NativeTokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<Duration>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Error)]
enum OAuthTransportError {
    #[error("invalid HTTP method '{method}' in oauth request")]
    InvalidMethod { method: String },
    #[error("invalid oauth request header name '{name}': {message}")]
    InvalidRequestHeaderName { name: String, message: String },
    #[error("invalid oauth request header value for '{name}': {message}")]
    InvalidRequestHeaderValue { name: String, message: String },
    #[error("failed to build oauth HTTP request")]
    BuildRequest(#[source] reqwest::Error),
    #[error("oauth HTTP request failed")]
    ExecuteRequest(#[source] reqwest::Error),
    #[error("failed to read oauth HTTP response body")]
    ReadResponseBody(#[source] reqwest::Error),
    #[error("invalid oauth response status code '{status}'")]
    InvalidStatusCode { status: u16 },
    #[error("invalid oauth response header name '{name}': {message}")]
    InvalidResponseHeaderName { name: String, message: String },
    #[error("invalid oauth response header value for '{name}': {message}")]
    InvalidResponseHeaderValue { name: String, message: String },
}

fn build_basic_client(
    client_id: &str,
    auth_url: &str,
    redirect_url: RedirectUrl,
    token_url: Option<TokenUrl>,
) -> Result<oauth2::basic::BasicClient> {
    Ok(oauth2::basic::BasicClient::new(
        ClientId::new(client_id.to_string()),
        None,
        AuthUrl::new(auth_url.to_string()).context("invalid auth_url (RFC 6749 §3.1)")?,
        token_url,
    )
    .set_redirect_uri(redirect_url))
}

fn build_authorization_url_with_client(
    client: oauth2::basic::BasicClient,
    scopes: &[String],
) -> (String, CsrfToken, PkceCodeVerifier) {
    let pkce = generate_pkce_pair();
    let mut builder = client
        .authorize_url(CsrfToken::new_random)
        .set_pkce_challenge(pkce.challenge);

    for scope in scopes {
        builder = builder.add_scope(Scope::new(scope.clone()));
    }

    let (url, csrf) = builder.url();
    (url.to_string(), csrf, pkce.verifier)
}

async fn execute_oauth_http_request(
    http: reqwest::Client,
    request: HttpRequest,
) -> std::result::Result<HttpResponse, OAuthTransportError> {
    let revised_request_url = crate::runtime::rewrite_url_for_logs(request.url.as_str());

    tracing::debug!(
        target: "vex::http",
        method = %request.method,
        url = %revised_request_url,
        "sending oauth request"
    );

    let method = reqwest::Method::from_bytes(request.method.as_str().as_bytes()).map_err(|_| {
        OAuthTransportError::InvalidMethod {
            method: request.method.to_string(),
        }
    })?;

    let mut request_builder = http
        .request(method, request.url.as_str())
        .body(request.body);
    for (name, value) in &request.headers {
        let request_name = reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes())
            .map_err(|error| OAuthTransportError::InvalidRequestHeaderName {
                name: name.as_str().to_string(),
                message: error.to_string(),
            })?;
        let request_value =
            reqwest::header::HeaderValue::from_bytes(value.as_bytes()).map_err(|error| {
                OAuthTransportError::InvalidRequestHeaderValue {
                    name: name.as_str().to_string(),
                    message: error.to_string(),
                }
            })?;
        request_builder = request_builder.header(request_name, request_value);
    }

    let request = request_builder
        .build()
        .map_err(OAuthTransportError::BuildRequest)?;
    let response = http
        .execute(request)
        .await
        .map_err(OAuthTransportError::ExecuteRequest)?;
    let status = response.status();
    let status_code = oauth2::http::StatusCode::from_u16(status.as_u16()).map_err(|_| {
        OAuthTransportError::InvalidStatusCode {
            status: status.as_u16(),
        }
    })?;
    let response_headers = response.headers().clone();
    let body = response
        .bytes()
        .await
        .map_err(OAuthTransportError::ReadResponseBody)?
        .to_vec();

    let mut headers = oauth2::http::HeaderMap::new();
    for (name, value) in &response_headers {
        let response_name = oauth2::http::header::HeaderName::from_bytes(name.as_str().as_bytes())
            .map_err(|error| OAuthTransportError::InvalidResponseHeaderName {
                name: name.as_str().to_string(),
                message: error.to_string(),
            })?;
        let response_value = oauth2::http::header::HeaderValue::from_bytes(value.as_bytes())
            .map_err(|error| OAuthTransportError::InvalidResponseHeaderValue {
                name: name.as_str().to_string(),
                message: error.to_string(),
            })?;
        headers.append(response_name, response_value);
    }

    Ok(HttpResponse {
        status_code,
        headers,
        body,
    })
}

fn validate_native_loopback_redirect_url(redirect_url: &str) -> Result<RedirectUrl> {
    let redirect = RedirectUrl::new(redirect_url.to_string())
        .context("invalid redirect_url (RFC 8252 §7.3)")?;
    let parsed = redirect.url();
    if parsed.scheme() != "http" {
        bail!("native redirect_url must use an http loopback redirect (RFC 8252 §7.3)");
    }

    let host = parsed
        .host_str()
        .context("native redirect_url must include a host (RFC 8252 §7.3)")?;
    let ip = host.parse::<std::net::IpAddr>().with_context(|| {
        format!("native redirect_url host '{host}' must be a loopback IP literal (RFC 8252 §7.3)")
    })?;
    if !ip.is_loopback() {
        bail!("native redirect_url host '{host}' must be a loopback IP literal (RFC 8252 §7.3)");
    }
    if parsed.port().is_none() {
        bail!("native redirect_url must include an explicit loopback port (RFC 8252 §7.3)");
    }

    Ok(redirect)
}

fn open_authorization_url_with<F, E>(authorization_url: &str, opener: F) -> Result<()>
where
    F: FnOnce(&str) -> std::result::Result<(), E>,
    E: std::error::Error + Send + Sync + 'static,
{
    let auth_url = AuthUrl::new(authorization_url.to_string())
        .context("invalid authorization_url (RFC 6749 §3.1)")?;
    opener(auth_url.url().as_str()).context("failed to open authorization_url in browser")
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
    let redirect_url = RedirectUrl::new(req.redirect_url.clone())
        .context("invalid redirect_url (RFC 6749 §3.1.2)")?;
    let client = build_basic_client(&req.client_id, &req.auth_url, redirect_url, None)?;
    Ok(build_authorization_url_with_client(client, &req.scopes))
}

/// Build a native-app authorization URL using a loopback redirect URI
/// (RFC 8252 §7.3 + RFC 7636 §4.3).
pub fn build_native_authorization_url(
    req: &NativeAuthorizationRequest,
) -> Result<(String, CsrfToken, PkceCodeVerifier)> {
    let redirect_url = validate_native_loopback_redirect_url(&req.redirect_url)?;
    let client = build_basic_client(&req.client_id, &req.auth_url, redirect_url, None)?;
    Ok(build_authorization_url_with_client(client, &req.scopes))
}

/// Open the system browser for the user-facing authorization URL
/// (RFC 8252 §4.1).
pub fn open_authorization_url(authorization_url: &str) -> Result<()> {
    open_authorization_url_with(authorization_url, |url| open::that(url).map(|_| ()))
}

/// Exchange a native-app authorization code using PKCE
/// (RFC 8252 §8.1 + RFC 6749 §4.1.3).
pub async fn exchange_native_authorization_code(
    req: &NativeCodeExchangeRequest,
) -> Result<NativeTokenResponse> {
    let redirect_url = validate_native_loopback_redirect_url(&req.redirect_url)?;
    let token_url =
        TokenUrl::new(req.token_url.clone()).context("invalid token_url (RFC 6749 §3.2)")?;
    let client = build_basic_client(&req.client_id, &req.auth_url, redirect_url, Some(token_url))?;
    let http = crate::net::http_client::default_client_builder(false)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("failed to build oauth HTTP client")?;
    let response = client
        .exchange_code(AuthorizationCode::new(req.authorization_code.clone()))
        .set_pkce_verifier(PkceCodeVerifier::new(req.pkce_verifier.clone()))
        .request_async(move |request| execute_oauth_http_request(http.clone(), request))
        .await
        .context("authorization code exchange failed (RFC 6749 §4.1.3 / RFC 8252 §8.1)")?;

    Ok(NativeTokenResponse {
        access_token: response.access_token().secret().to_string(),
        refresh_token: response
            .refresh_token()
            .map(|refresh_token| refresh_token.secret().to_string()),
        expires_in: response.expires_in(),
        scopes: response
            .scopes()
            .map(|scopes| {
                scopes
                    .iter()
                    .map(|scope| scope.as_str().to_string())
                    .collect()
            })
            .unwrap_or_default(),
    })
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
    use axum::Router;
    use axum::body::Bytes;
    use axum::http::{StatusCode, header::CONTENT_TYPE};
    use axum::routing::post;
    use std::sync::{Arc, Mutex};
    use tokio::sync::Mutex as AsyncMutex;

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

    #[test]
    fn build_native_authorization_url_rejects_non_loopback_redirect_rfc8252() {
        let req = NativeAuthorizationRequest {
            client_id: "native-client".to_string(),
            auth_url: "https://auth.example.com/authorize".to_string(),
            redirect_url: "https://app.example.com/callback".to_string(),
            scopes: vec!["repo".to_string()],
        };

        let error = build_native_authorization_url(&req).unwrap_err();
        assert!(
            error.to_string().contains("loopback"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn open_authorization_url_uses_injected_opener_rfc8252() {
        let seen = Arc::new(Mutex::new(None::<String>));
        let seen_for_opener = Arc::clone(&seen);

        open_authorization_url_with("https://auth.example.com/authorize", move |url| {
            *seen_for_opener.lock().expect("opener mutex") = Some(url.to_string());
            Ok::<(), std::io::Error>(())
        })
        .expect("open authorization url");

        assert_eq!(
            seen.lock().expect("seen mutex").as_deref(),
            Some("https://auth.example.com/authorize")
        );
    }

    #[tokio::test]
    async fn exchange_native_authorization_code_posts_pkce_rfc8252() {
        let captured_body = Arc::new(AsyncMutex::new(None::<String>));
        let captured_body_for_route = Arc::clone(&captured_body);
        let app = Router::new().route(
            "/token",
            post(move |body: Bytes| {
                let captured_body_for_route = Arc::clone(&captured_body_for_route);
                async move {
                    *captured_body_for_route.lock().await = Some(
                        String::from_utf8(body.to_vec()).expect("token request body should be utf-8"),
                    );
                    (
                        StatusCode::OK,
                        [(CONTENT_TYPE, "application/json")],
                        r#"{"access_token":"access-123","token_type":"Bearer","refresh_token":"refresh-123","expires_in":3600,"scope":"repo gist"}"#,
                    )
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind token server");
        let addr = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve token endpoint");
        });

        let pkce = generate_pkce_pair();
        let verifier_secret = pkce.verifier.secret().to_string();
        let response = exchange_native_authorization_code(&NativeCodeExchangeRequest {
            client_id: "native-client".to_string(),
            auth_url: format!("http://127.0.0.1:{}/authorize", addr.port()),
            token_url: format!("http://127.0.0.1:{}/token", addr.port()),
            redirect_url: "http://127.0.0.1:8123/callback".to_string(),
            authorization_code: "auth-code".to_string(),
            pkce_verifier: verifier_secret.clone(),
        })
        .await
        .expect("exchange authorization code");

        assert_eq!(response.access_token, "access-123");
        assert_eq!(response.refresh_token.as_deref(), Some("refresh-123"));
        assert_eq!(response.expires_in, Some(Duration::from_secs(3600)));
        assert_eq!(
            response.scopes,
            vec!["repo".to_string(), "gist".to_string()]
        );

        let captured_body = captured_body
            .lock()
            .await
            .clone()
            .expect("captured token request body");
        assert!(captured_body.contains("grant_type=authorization_code"));
        assert!(captured_body.contains("code=auth-code"));
        assert!(captured_body.contains(&format!("code_verifier={verifier_secret}")));
        assert!(
            captured_body.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8123%2Fcallback"),
            "unexpected body: {captured_body}"
        );

        server.abort();
    }
}
