use crate::config::Config;
use crate::local_api::{ActiveTask, LocalApiState, LocalApiTaskShared};

use super::HttpSurfaceSettings;
use super::http::{build_http_router, build_router};
use super::sse::{TurnsSseMode, negotiate_turns_sse_mode, runtime_sse_response};
use super::util::resolve_serve_config;

use crate::app::runtime_tokio::{
    sync::{Mutex as AsyncMutex, mpsc},
    time::timeout,
};
use crate::http_facade::header::{AUTHORIZATION, CONTENT_TYPE};
use crate::http_facade::{Request, StatusCode};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;

mod core;
mod graph;
mod phase_e;
mod teams;
mod watch;
