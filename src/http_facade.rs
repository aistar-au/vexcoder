// Keep upgrade-sensitive `http` / `axum::http` types behind one local seam.
// Handler extractors and routers stay in server modules because those APIs are
// framework-specific, but header/status/request types should not leak broadly.

pub mod header {
    #[allow(unused_imports)]
    pub use http::header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, STRICT_TRANSPORT_SECURITY};
}

pub use axum::extract::Request;
pub use http::{HeaderName, HeaderValue, StatusCode};
