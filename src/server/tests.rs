use crate::config::Config;
use crate::local_api::{ActiveTask, LocalApiState, LocalApiTaskShared};
use crate::runtime::json_handoff::RuntimeEnvelopeNormalizer;

use super::HttpSurfaceSettings;
use super::http::{build_http_router, build_router};
use super::sse::runtime_sse_response;
use super::util::resolve_serve_config;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tower::ServiceExt;

mod core;
mod graph;
mod phase_e;
mod teams;
mod watch;
