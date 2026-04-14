use crate::config::Config;
use crate::local_api::{ActiveTask, LocalApiState, LocalApiTaskShared};
use crate::runtime::json_handoff::RuntimeEnvelopeNormalizer;

use super::http::{build_http_router, build_router};
use super::sse::runtime_sse_response;
use super::util::resolve_serve_config;
use super::HttpSurfaceSettings;

use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::timeout;
use tower::ServiceExt;

mod core;
mod graph;
mod phase_e;
mod teams;
mod watch;
