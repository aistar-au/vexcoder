use crate::config::Config;
use crate::local_api::LocalApiState;

use super::HttpSurfaceSettings;
use super::http::{build_http_router, build_router};
use super::util::resolve_serve_config;

use crate::http_facade::header::{CACHE_CONTROL, CONTENT_TYPE};
use crate::http_facade::{Request, StatusCode};
use axum::body::{Body, to_bytes};
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

mod core;
mod graph;
mod phase_e;
mod teams;
mod watch;
