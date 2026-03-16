use crate::api::ApiClient;
use crate::config::Config;
use crate::runtime::context::RuntimeContext;
use crate::runtime::frontend::{FrontendAdapter, UserInputEvent};
use crate::runtime::json_handoff::{
    runtime_approval_request_event, RuntimeEnvelope, RuntimeEnvelopeNormalizer, RuntimeEvent,
    RuntimeRequest, TurnEndContext,
};
use crate::runtime::mode::RuntimeMode;
use crate::runtime::project_instructions::{load_project_instructions, LoadResult};
use crate::runtime::r#loop::Runtime;
use crate::runtime::UiUpdate;
use crate::session_notes::build_api_client_with_notes;
use crate::state::{ConversationManager, TurnToolPolicy};
use crate::tools::ToolOperator;
use anyhow::{anyhow, bail, Context, Result};
use axum::extract::{Request, State};
use axum::http::header::{AUTHORIZATION, STRICT_TRANSPORT_SECURITY};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as HyperConnectionBuilder;
use hyper_util::service::TowerToHyperService;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::convert::Infallible;
use std::io::BufReader;
use std::net::ToSocketAddrs;
#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

pub const DEFAULT_LOCAL_API_PORT: u16 = 6274;

const HSTS_HEADER_VALUE: &str = "max-age=31536000";
const SSE_KEEPALIVE_TEXT: &str = "keepalive";
const SSE_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct LocalApiState {
    config: Config,
    tasks: Arc<AsyncMutex<HashMap<String, ActiveTask>>>,
}

struct ActiveTask {
    interrupt_tx: mpsc::UnboundedSender<FrontendCommand>,
    shared: Arc<Mutex<LocalApiTaskShared>>,
}

struct LocalApiTaskShared {
    normalizer: RuntimeEnvelopeNormalizer,
    envelope_tx: mpsc::UnboundedSender<String>,
    pending_approval: Option<PendingApproval>,
    quit: Arc<AtomicBool>,
    turn_in_progress: bool,
    interrupted: bool,
}

struct PendingApproval {
    capability: String,
    scope: String,
    response_tx: tokio::sync::oneshot::Sender<bool>,
}

#[derive(Clone, Debug)]
struct HttpSurfaceSettings {
    bearer_token: Arc<str>,
    hsts_enabled: bool,
}

#[derive(Debug)]
struct ResolvedServeConfig {
    http: Option<ResolvedHttpSurface>,
    unix: Option<ResolvedUnixSurface>,
}

#[derive(Debug)]
struct ResolvedHttpSurface {
    bind_addr: String,
    port: u16,
    auth: HttpSurfaceSettings,
    tls: Option<Arc<rustls::ServerConfig>>,
}

#[derive(Debug)]
#[cfg_attr(not(unix), allow(dead_code))]
struct ResolvedUnixSurface {
    socket_path: PathBuf,
}

enum FrontendCommand {
    Interrupt,
}

struct LocalApiFrontend {
    initial_input: Option<String>,
    command_rx: mpsc::UnboundedReceiver<FrontendCommand>,
    quit: Arc<AtomicBool>,
}

impl LocalApiFrontend {
    fn new(
        initial_input: String,
        command_rx: mpsc::UnboundedReceiver<FrontendCommand>,
        quit: Arc<AtomicBool>,
    ) -> Self {
        Self {
            initial_input: Some(initial_input),
            command_rx,
            quit,
        }
    }
}

impl FrontendAdapter<LocalApiMode> for LocalApiFrontend {
    fn poll_user_input(&mut self, mode: &LocalApiMode) -> Option<UserInputEvent> {
        if let Ok(command) = self.command_rx.try_recv() {
            return match command {
                FrontendCommand::Interrupt => Some(UserInputEvent::Interrupt),
            };
        }

        if !mode.is_turn_in_progress() {
            return self.initial_input.take().map(UserInputEvent::Text);
        }

        None
    }

    fn render(&mut self, _mode: &LocalApiMode) {}

    fn should_quit(&self) -> bool {
        self.quit.load(Ordering::SeqCst)
    }
}

struct LocalApiMode {
    shared: Arc<Mutex<LocalApiTaskShared>>,
}

impl LocalApiMode {
    fn new(shared: Arc<Mutex<LocalApiTaskShared>>) -> Self {
        Self { shared }
    }

    fn emit_envelopes(shared: &mut LocalApiTaskShared, envelopes: Vec<RuntimeEnvelope>) {
        for envelope in envelopes {
            if let Ok(json) = serde_json::to_string(&envelope) {
                let _ = shared.envelope_tx.send(json);
            }
        }
    }
}

impl RuntimeMode for LocalApiMode {
    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext) {
        let mut shared = self.shared.lock().expect("local api shared lock poisoned");
        shared.turn_in_progress = true;
        shared.interrupted = false;
        let start = shared.normalizer.start_turn(1, Some(input.clone()));
        Self::emit_envelopes(&mut shared, vec![start]);
        drop(shared);

        ctx.start_turn_with_system_prompt_and_policy(input, None, TurnToolPolicy::Default);
    }

    fn on_interrupt(&mut self, ctx: &mut RuntimeContext) {
        let mut shared = self.shared.lock().expect("local api shared lock poisoned");
        if !shared.turn_in_progress {
            return;
        }
        shared.interrupted = true;
        drop(shared);
        ctx.cancel_turn();
    }

    fn on_model_update(&mut self, update: UiUpdate, _ctx: &mut RuntimeContext) {
        let mut shared = self.shared.lock().expect("local api shared lock poisoned");
        match update {
            UiUpdate::TranscriptLine(_) => {}
            UiUpdate::StreamDelta(text) => {
                let envelopes = shared
                    .normalizer
                    .normalize_ui_update(&UiUpdate::StreamDelta(text), None);
                Self::emit_envelopes(&mut shared, envelopes);
            }
            UiUpdate::StreamBlockStart { block, .. } => {
                let envelopes = shared.normalizer.normalize_stream_block(&block);
                Self::emit_envelopes(&mut shared, envelopes);
            }
            UiUpdate::StreamBlockDelta { .. } | UiUpdate::StreamBlockComplete { .. } => {}
            UiUpdate::ToolApprovalRequest(request) => {
                let event = runtime_approval_request_event(&request);
                let (capability, scope) = match &event {
                    RuntimeEvent::ApprovalRequest {
                        capability, scope, ..
                    } => (capability.clone(), scope.clone()),
                    _ => ("unknown".to_string(), "once".to_string()),
                };
                shared.pending_approval = Some(PendingApproval {
                    capability,
                    scope,
                    response_tx: request.response_tx,
                });
                let envelope = shared.normalizer.emit_event(event);
                Self::emit_envelopes(&mut shared, vec![envelope]);
            }
            UiUpdate::TurnComplete => {
                let envelopes = if shared.interrupted {
                    shared.normalizer.emit_cancelled(TurnEndContext::default())
                } else {
                    shared.normalizer.normalize_ui_update(
                        &UiUpdate::TurnComplete,
                        Some(TurnEndContext::default()),
                    )
                };
                shared.turn_in_progress = false;
                shared.quit.store(true, Ordering::SeqCst);
                Self::emit_envelopes(&mut shared, envelopes);
            }
            UiUpdate::Error(message) => {
                let envelopes = shared.normalizer.emit_error(
                    "runtime_error".to_string(),
                    message,
                    false,
                    TurnEndContext::default(),
                );
                shared.turn_in_progress = false;
                shared.quit.store(true, Ordering::SeqCst);
                Self::emit_envelopes(&mut shared, envelopes);
            }
            UiUpdate::CommandSessionStarted { .. }
            | UiUpdate::CommandSessionAttached { .. }
            | UiUpdate::EditLoopComplete { .. }
            | UiUpdate::CommandSessionFinished { .. } => {}
        }
    }

    fn is_turn_in_progress(&self) -> bool {
        self.shared
            .lock()
            .expect("local api shared lock poisoned")
            .turn_in_progress
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    ok: bool,
    service: &'static str,
    version: u16,
}

#[derive(Debug, Serialize)]
struct ControlResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct SchemaBundle {
    version: u16,
    request_schema: Value,
    envelope_schema: Value,
}

impl LocalApiState {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            tasks: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }
}

pub fn build_router(config: Config) -> Router {
    build_router_with_state(LocalApiState::new(config))
}

fn build_router_with_state(state: LocalApiState) -> Router {
    Router::new()
        .route("/v1/health", get(health_handler))
        .route("/v1/schema", get(schema_handler))
        .route("/v1/turns", post(turns_handler))
        .route("/v1/interrupt", post(interrupt_handler))
        .route("/v1/approve", post(approve_handler))
        .with_state(state)
}

fn build_http_router(state: LocalApiState, auth: HttpSurfaceSettings) -> Router {
    let expected_header = Arc::<str>::from(format!("Bearer {}", auth.bearer_token));
    let hsts_enabled = auth.hsts_enabled;
    build_router_with_state(state).layer(middleware::from_fn(move |request, next| {
        let expected_header = Arc::clone(&expected_header);
        async move { authorize_http_request(request, next, expected_header, hsts_enabled).await }
    }))
}

async fn authorize_http_request(
    request: Request,
    next: Next,
    expected_header: Arc<str>,
    hsts_enabled: bool,
) -> Response {
    let provided = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim);
    if provided != Some(expected_header.as_ref()) {
        return unauthorized_response();
    }

    let mut response = next.run(request).await;
    if hsts_enabled {
        response.headers_mut().insert(
            STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static(HSTS_HEADER_VALUE),
        );
    }
    response
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(ControlResponse {
            ok: false,
            reason: Some("unauthorized"),
        }),
    )
        .into_response()
}

pub async fn serve_local_api(
    config: Config,
    host_override: Option<String>,
    port_override: Option<u16>,
) -> Result<()> {
    let resolved = resolve_serve_config(&config, host_override, port_override)?;
    let state = LocalApiState::new(config);
    let shutdown = CancellationToken::new();
    let shutdown_signal = shutdown.clone();
    let signal_task = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            shutdown_signal.cancel();
        }
    });
    let mut servers = JoinSet::new();

    if let Some(http) = resolved.http {
        let router = build_http_router(state.clone(), http.auth.clone());
        servers.spawn(run_http_surface(router, http, shutdown.clone()));
    }
    if let Some(unix) = resolved.unix {
        #[cfg(unix)]
        {
            let router = build_router_with_state(state.clone());
            servers.spawn(run_unix_surface(router, unix, shutdown.clone()));
        }
        #[cfg(not(unix))]
        {
            let _ = unix;
            bail!("Unix-socket transport is only supported on macOS and Linux");
        }
    }

    let mut first_error: Option<anyhow::Error> = None;
    while let Some(result) = servers.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                first_error = Some(error);
                shutdown.cancel();
                break;
            }
            Err(error) => {
                first_error = Some(anyhow!("LocalApiServer task join error: {error}"));
                shutdown.cancel();
                break;
            }
        }
    }

    shutdown.cancel();
    while let Some(result) = servers.join_next().await {
        if let Ok(Err(error)) = result {
            first_error.get_or_insert(error);
        }
    }
    signal_task.abort();

    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(())
}

fn resolve_serve_config(
    config: &Config,
    host_override: Option<String>,
    port_override: Option<u16>,
) -> Result<ResolvedServeConfig> {
    if config.api.tls_skip_verify {
        bail!("api.tls_skip_verify must remain false in Phase I");
    }
    if config.api.vpn_trust {
        bail!("api.vpn_trust must remain false until a dedicated ADR exists");
    }

    let mut resolved = ResolvedServeConfig {
        http: None,
        unix: None,
    };
    let transport = config.api.transport;

    if transport.http_enabled() {
        let bind_addr = host_override.unwrap_or_else(|| config.api.host.clone());
        let port = port_override.unwrap_or(config.api.port);
        let bearer_token = config
            .api
            .key
            .clone()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                anyhow!("LocalApiServer HTTP transport requires VEX_API_KEY or api.key")
            })?;
        let is_loopback = is_strict_loopback_host(&bind_addr, port)?;
        let tls = build_http_tls_config(config, is_loopback)?;
        resolved.http = Some(ResolvedHttpSurface {
            bind_addr,
            port,
            auth: HttpSurfaceSettings {
                bearer_token: Arc::<str>::from(bearer_token),
                hsts_enabled: tls.is_some(),
            },
            tls,
        });
    }

    if transport.unix_enabled() {
        #[cfg(unix)]
        {
            resolved.unix = Some(ResolvedUnixSurface {
                socket_path: config
                    .api
                    .socket
                    .clone()
                    .unwrap_or_else(default_unix_socket_path),
            });
        }
        #[cfg(not(unix))]
        {
            bail!("Unix-socket transport is only supported on macOS and Linux");
        }
    }

    Ok(resolved)
}

async fn run_http_surface(
    router: Router,
    surface: ResolvedHttpSurface,
    shutdown: CancellationToken,
) -> Result<()> {
    let listener = tokio::net::TcpListener::bind((surface.bind_addr.as_str(), surface.port))
        .await
        .with_context(|| {
            format!(
                "failed to bind LocalApiServer on {}:{}",
                surface.bind_addr, surface.port
            )
        })?;

    match surface.tls {
        Some(tls_config) => serve_tls_listener(listener, router, tls_config, shutdown).await,
        None => axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                shutdown.cancelled().await;
            })
            .await
            .context("LocalApiServer exited with an error"),
    }
}

async fn serve_tls_listener(
    listener: tokio::net::TcpListener,
    router: Router,
    tls_config: Arc<rustls::ServerConfig>,
    shutdown: CancellationToken,
) -> Result<()> {
    let acceptor = TlsAcceptor::from(tls_config);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            accepted = listener.accept() => {
                let (stream, _) = accepted.context("failed to accept LocalApiServer TLS connection")?;
                let acceptor = acceptor.clone();
                let service = TowerToHyperService::new(router.clone());
                let shutdown = shutdown.clone();
                tokio::spawn(async move {
                    let tls_stream = match acceptor.accept(stream).await {
                        Ok(stream) => stream,
                        Err(error) => {
                            eprintln!("[local api] tls accept failed: {error}");
                            return;
                        }
                    };
                    let io = TokioIo::new(tls_stream);
                    let builder = HyperConnectionBuilder::new(TokioExecutor::new());
                    let connection = builder.serve_connection(io, service);
                    tokio::select! {
                        _ = shutdown.cancelled() => {}
                        result = connection => {
                            if let Err(error) = result {
                                eprintln!("[local api] tls connection error: {error}");
                            }
                        }
                    }
                });
            }
        }
    }
}

fn is_strict_loopback_host(host: &str, port: u16) -> Result<bool> {
    let normalized = host.trim().to_ascii_lowercase();
    if let Ok(addr) = normalized.parse::<std::net::IpAddr>() {
        return Ok(addr.is_loopback());
    }
    if normalized != "localhost" {
        return Ok(false);
    }

    let addrs = (host, port)
        .to_socket_addrs()
        .with_context(|| format!("failed to resolve LocalApiServer host '{host}'"))?
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        bail!("failed to resolve LocalApiServer host '{host}'");
    }
    Ok(addrs.iter().all(|addr| addr.ip().is_loopback()))
}

fn build_http_tls_config(
    config: &Config,
    is_loopback: bool,
) -> Result<Option<Arc<rustls::ServerConfig>>> {
    if let Some(ca_path) = config.api.tls_ca_cert.as_deref() {
        validate_pem_certificates(ca_path)
            .with_context(|| format!("invalid api.tls_ca_cert '{}'", ca_path.display()))?;
    }

    let cert_path = config.api.tls_cert.as_deref();
    let key_path = config.api.tls_key.as_deref();
    let tls_requested = cert_path.is_some() || key_path.is_some();

    if !is_loopback && !tls_requested {
        bail!("LocalApiServer non-loopback HTTP bind requires both api.tls_cert and api.tls_key");
    }
    if !tls_requested {
        return Ok(None);
    }

    let cert_path =
        cert_path.ok_or_else(|| anyhow!("api.tls_cert is required when TLS is enabled"))?;
    let key_path =
        key_path.ok_or_else(|| anyhow!("api.tls_key is required when TLS is enabled"))?;
    let cert_chain = load_pem_certificates(cert_path)
        .with_context(|| format!("invalid api.tls_cert '{}'", cert_path.display()))?;
    let private_key = load_private_key(key_path)
        .with_context(|| format!("invalid api.tls_key '{}'", key_path.display()))?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut tls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)
        .context(
            "api.tls_cert and api.tls_key must form a matching certificate/private-key pair",
        )?;
    tls_config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Some(Arc::new(tls_config)))
}

fn load_pem_certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to read PEM certificates from '{}'", path.display()))?;
    let mut reader = BufReader::new(file);
    let certificates = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to parse PEM certificates from '{}'", path.display()))?;
    if certificates.is_empty() {
        bail!("no PEM certificates found in '{}'", path.display());
    }
    Ok(certificates)
}

fn validate_pem_certificates(path: &Path) -> Result<()> {
    let _ = load_pem_certificates(path)?;
    Ok(())
}

fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let pkcs8 = load_first_private_key(path, PrivateKeyFormat::Pkcs8)?;
    if let Some(key) = pkcs8 {
        return Ok(key);
    }

    let sec1 = load_first_private_key(path, PrivateKeyFormat::Sec1)?;
    if let Some(key) = sec1 {
        return Ok(key);
    }

    let rsa = load_first_private_key(path, PrivateKeyFormat::Rsa)?;
    if let Some(key) = rsa {
        return Ok(key);
    }

    bail!("no supported PEM private key found in '{}'", path.display())
}

enum PrivateKeyFormat {
    Pkcs8,
    Sec1,
    Rsa,
}

fn load_first_private_key(
    path: &Path,
    format: PrivateKeyFormat,
) -> Result<Option<PrivateKeyDer<'static>>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to read PEM private key from '{}'", path.display()))?;
    let mut reader = BufReader::new(file);
    let key = match format {
        PrivateKeyFormat::Pkcs8 => rustls_pemfile::pkcs8_private_keys(&mut reader)
            .next()
            .transpose()?
            .map(Into::into),
        PrivateKeyFormat::Sec1 => rustls_pemfile::ec_private_keys(&mut reader)
            .next()
            .transpose()?
            .map(Into::into),
        PrivateKeyFormat::Rsa => rustls_pemfile::rsa_private_keys(&mut reader)
            .next()
            .transpose()?
            .map(Into::into),
    };
    Ok(key)
}

#[cfg(unix)]
fn default_unix_socket_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("vexcoder.sock")
}

#[cfg(unix)]
async fn run_unix_surface(
    router: Router,
    surface: ResolvedUnixSurface,
    shutdown: CancellationToken,
) -> Result<()> {
    let listener = bind_unix_listener(&surface.socket_path)?;
    let cleanup_path = surface.socket_path.clone();
    let result = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            shutdown.cancelled().await;
        })
        .await
        .context("LocalApiServer unix socket exited with an error");
    remove_unix_socket(&cleanup_path)?;
    result
}

#[cfg(unix)]
fn bind_unix_listener(path: &Path) -> Result<tokio::net::UnixListener> {
    if path.exists() {
        let metadata = std::fs::symlink_metadata(path)
            .with_context(|| format!("failed to inspect unix socket '{}'", path.display()))?;
        if metadata.file_type().is_socket() {
            std::fs::remove_file(path).with_context(|| {
                format!("failed to remove stale unix socket '{}'", path.display())
            })?;
        } else {
            bail!(
                "refusing to replace non-socket path '{}'; configure api.socket to a unix socket path",
                path.display()
            );
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create unix socket parent directory '{}'",
                parent.display()
            )
        })?;
    }

    let listener = tokio::net::UnixListener::bind(path)
        .with_context(|| format!("failed to bind unix socket '{}'", path.display()))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).with_context(|| {
        format!(
            "failed to apply 0600 permissions to unix socket '{}'",
            path.display()
        )
    })?;
    Ok(listener)
}

#[cfg(unix)]
fn remove_unix_socket(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path)
            .with_context(|| format!("failed to remove unix socket '{}'", path.display()))?;
    }
    Ok(())
}

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        service: "vexcoder-local-api",
        version: 1,
    })
}

async fn schema_handler() -> Result<Json<SchemaBundle>, (StatusCode, Json<ControlResponse>)> {
    let request_schema: Value =
        serde_json::from_str(include_str!("../schemas/runtime_request_v1.json"))
            .map_err(internal_error)?;
    let envelope_schema: Value =
        serde_json::from_str(include_str!("../schemas/runtime_envelope_v1.json"))
            .map_err(internal_error)?;
    Ok(Json(SchemaBundle {
        version: 1,
        request_schema,
        envelope_schema,
    }))
}

async fn turns_handler(
    State(state): State<LocalApiState>,
    Json(request): Json<RuntimeRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ControlResponse>)> {
    let RuntimeRequest::SubmitInput { task_id, input } = request else {
        return Err(bad_request("invalid_request_type"));
    };

    let task_id = task_id.unwrap_or_else(new_server_task_id);
    let (envelope_tx, envelope_rx) = mpsc::unbounded_channel::<String>();
    let (interrupt_tx, interrupt_rx) = mpsc::unbounded_channel::<FrontendCommand>();
    let quit = Arc::new(AtomicBool::new(false));
    let shared = Arc::new(Mutex::new(LocalApiTaskShared {
        normalizer: RuntimeEnvelopeNormalizer::new(task_id.clone()),
        envelope_tx,
        pending_approval: None,
        quit: Arc::clone(&quit),
        turn_in_progress: false,
        interrupted: false,
    }));

    {
        let mut tasks = state.tasks.lock().await;
        if tasks.contains_key(&task_id) {
            return Err(conflict("task_already_active"));
        }
        tasks.insert(
            task_id.clone(),
            ActiveTask {
                interrupt_tx,
                shared: Arc::clone(&shared),
            },
        );
    }

    spawn_local_api_task(state.clone(), task_id, input, shared, interrupt_rx);

    Ok((
        StatusCode::OK,
        [(axum::http::header::CACHE_CONTROL, "no-cache")],
        runtime_sse_response(envelope_rx, SSE_KEEPALIVE_INTERVAL),
    ))
}

fn runtime_sse_response(
    envelope_rx: mpsc::UnboundedReceiver<String>,
    keepalive_interval: Duration,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = UnboundedReceiverStream::new(envelope_rx)
        .map(|payload| Ok::<Event, Infallible>(Event::default().event("runtime").data(payload)));
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(keepalive_interval)
            .text(SSE_KEEPALIVE_TEXT),
    )
}

async fn interrupt_handler(
    State(state): State<LocalApiState>,
    Json(request): Json<RuntimeRequest>,
) -> Result<Json<ControlResponse>, (StatusCode, Json<ControlResponse>)> {
    let RuntimeRequest::Interrupt { task_id } = request else {
        return Err(bad_request("invalid_request_type"));
    };

    let tasks = state.tasks.lock().await;
    let Some(task) = tasks.get(&task_id) else {
        return Err(not_found("task_not_found"));
    };
    task.interrupt_tx
        .send(FrontendCommand::Interrupt)
        .map_err(|_| not_found("task_not_found"))?;

    Ok(Json(ControlResponse {
        ok: true,
        reason: None,
    }))
}

async fn approve_handler(
    State(state): State<LocalApiState>,
    Json(request): Json<RuntimeRequest>,
) -> Result<Json<ControlResponse>, (StatusCode, Json<ControlResponse>)> {
    let task_id = match &request {
        RuntimeRequest::ApproveCapability { task_id, .. }
        | RuntimeRequest::DenyCapability { task_id, .. } => task_id.clone(),
        _ => return Err(bad_request("invalid_request_type")),
    };

    let tasks = state.tasks.lock().await;
    let Some(task) = tasks.get(&task_id) else {
        return Err(not_found("task_not_found"));
    };

    let mut shared = task.shared.lock().expect("local api shared lock poisoned");
    let Some(pending) = shared.pending_approval.take() else {
        return Err(conflict("no_pending_approval"));
    };

    let capability = match &request {
        RuntimeRequest::ApproveCapability { capability, .. }
        | RuntimeRequest::DenyCapability { capability, .. } => capability,
        _ => unreachable!(),
    };
    if pending.capability != *capability {
        shared.pending_approval = Some(pending);
        return Err(conflict("no_pending_approval"));
    }

    if let RuntimeRequest::ApproveCapability { scope, .. } = &request {
        if pending.scope != *scope {
            shared.pending_approval = Some(pending);
            return Err(conflict("no_pending_approval"));
        }
    }

    let approved = matches!(request, RuntimeRequest::ApproveCapability { .. });
    let _ = pending.response_tx.send(approved);
    let envelopes = shared.normalizer.normalize_runtime_request(&request);
    LocalApiMode::emit_envelopes(&mut shared, envelopes);

    Ok(Json(ControlResponse {
        ok: true,
        reason: None,
    }))
}

fn spawn_local_api_task(
    state: LocalApiState,
    task_id: String,
    input: String,
    shared: Arc<Mutex<LocalApiTaskShared>>,
    interrupt_rx: mpsc::UnboundedReceiver<FrontendCommand>,
) {
    tokio::spawn(async move {
        let result = run_local_api_task(
            state.config.clone(),
            task_id.clone(),
            input,
            Arc::clone(&shared),
            interrupt_rx,
        )
        .await;
        if let Err(error) = result {
            let mut shared = shared.lock().expect("local api shared lock poisoned");
            let envelopes = shared.normalizer.emit_error(
                "local_api_server".to_string(),
                error.to_string(),
                false,
                TurnEndContext::default(),
            );
            shared.quit.store(true, Ordering::SeqCst);
            LocalApiMode::emit_envelopes(&mut shared, envelopes);
        }
        state.tasks.lock().await.remove(&task_id);
    });
}

async fn run_local_api_task(
    config: Config,
    task_id: String,
    input: String,
    shared: Arc<Mutex<LocalApiTaskShared>>,
    interrupt_rx: mpsc::UnboundedReceiver<FrontendCommand>,
) -> Result<()> {
    let client = build_local_api_client(&config)?;
    let operator = ToolOperator::new(config.working_dir.clone());
    let conversation = ConversationManager::new_with_hooks(client, operator, config.hooks.clone());
    let (update_tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
    let mode = LocalApiMode::new(Arc::clone(&shared));
    let quit = shared
        .lock()
        .expect("local api shared lock poisoned")
        .quit
        .clone();
    let mut frontend = LocalApiFrontend::new(input, interrupt_rx, quit);
    let mut runtime = Runtime::new(mode, update_rx);
    runtime.run(&mut frontend, &mut ctx).await;
    if !frontend.should_quit() {
        return Err(anyhow!(
            "local api runtime exited before signalling completion for {task_id}"
        ));
    }
    Ok(())
}

fn build_local_api_client(config: &Config) -> Result<ApiClient> {
    let instructions = match load_project_instructions(
        &config.working_dir,
        config.max_project_instructions_tokens,
    ) {
        LoadResult::Loaded(project_instructions) => Some(project_instructions.content),
        LoadResult::OverBudget {
            path,
            estimated_tokens,
        } => {
            eprintln!(
                "[project instructions] {} skipped: estimated {} tokens exceeds budget of {}",
                path.display(),
                estimated_tokens,
                config.max_project_instructions_tokens,
            );
            None
        }
        LoadResult::NotFound => None,
    };

    let (client, notes_warning) = build_api_client_with_notes(config)?;
    if let Some(warning) = notes_warning {
        eprintln!("{warning}");
    }
    Ok(client.with_project_instructions(instructions))
}

fn new_server_task_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("task-{millis}")
}

fn bad_request(reason: &'static str) -> (StatusCode, Json<ControlResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ControlResponse {
            ok: false,
            reason: Some(reason),
        }),
    )
}

fn not_found(reason: &'static str) -> (StatusCode, Json<ControlResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(ControlResponse {
            ok: false,
            reason: Some(reason),
        }),
    )
}

fn conflict(reason: &'static str) -> (StatusCode, Json<ControlResponse>) {
    (
        StatusCode::CONFLICT,
        Json(ControlResponse {
            ok: false,
            reason: Some(reason),
        }),
    )
}

fn internal_error(_: serde_json::Error) -> (StatusCode, Json<ControlResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ControlResponse {
            ok: false,
            reason: Some("internal_error"),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::mock_client::MockApiClient;
    use axum::body::to_bytes;
    use axum::body::Body;
    use axum::http::header::AUTHORIZATION;
    use axum::http::header::CONTENT_TYPE;
    use axum::http::Request;
    use tokio::time::timeout;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_endpoint_returns_ok() {
        let router = build_router(Config::default_for_tui());
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_schema_endpoint_returns_bundle() {
        let router = build_router(Config::default_for_tui());
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/schema")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_http_router_requires_bearer_token() {
        let mut config = Config::default_for_tui();
        config.api.key = Some("token-123".to_string());
        let router = build_http_router(
            LocalApiState::new(config),
            HttpSurfaceSettings {
                bearer_token: Arc::<str>::from("token-123"),
                hsts_enabled: false,
            },
        );

        let unauthorized = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let authorized = router
            .oneshot(
                Request::builder()
                    .uri("/v1/health")
                    .header(AUTHORIZATION, "Bearer token-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(authorized.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_http_router_rejects_invalid_bearer_token() {
        let mut config = Config::default_for_tui();
        config.api.key = Some("token-123".to_string());
        let router = build_http_router(
            LocalApiState::new(config),
            HttpSurfaceSettings {
                bearer_token: Arc::<str>::from("token-123"),
                hsts_enabled: false,
            },
        );

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/health")
                    .header(AUTHORIZATION, "Bearer wrong-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.get("ok"), Some(&Value::Bool(false)));
        assert_eq!(
            payload.get("reason"),
            Some(&Value::String("unauthorized".to_string()))
        );
    }

    #[tokio::test]
    async fn test_runtime_sse_response_emits_keepalive_comment() {
        #[derive(Clone)]
        struct TestSseState {
            _sender: mpsc::UnboundedSender<String>,
            receiver: Arc<AsyncMutex<Option<mpsc::UnboundedReceiver<String>>>>,
        }

        async fn keepalive_handler(State(state): State<TestSseState>) -> impl IntoResponse {
            let receiver = state
                .receiver
                .lock()
                .await
                .take()
                .expect("single keepalive request");
            runtime_sse_response(receiver, Duration::from_millis(20))
        }

        let (sender, receiver) = mpsc::unbounded_channel::<String>();
        let state = TestSseState {
            _sender: sender,
            receiver: Arc::new(AsyncMutex::new(Some(receiver))),
        };
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/", get(keepalive_handler))
                    .with_state(state),
            )
            .await
            .unwrap();
        });

        let response = reqwest::get(format!("http://{addr}/")).await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let mut stream = response.bytes_stream();
        let chunk = timeout(
            Duration::from_secs(1),
            futures::StreamExt::next(&mut stream),
        )
        .await
        .expect("keepalive chunk timed out")
        .expect("stream ended unexpectedly")
        .unwrap();
        let payload = String::from_utf8_lossy(&chunk);
        assert!(
            payload.contains(": keepalive"),
            "expected SSE keepalive comment, got {payload:?}"
        );

        server.abort();
    }

    #[tokio::test]
    async fn test_interrupt_handler_returns_not_found_for_unknown_task() {
        let mut config = Config::default_for_tui();
        config.api.key = Some("token-123".to_string());
        let router = build_http_router(
            LocalApiState::new(config),
            HttpSurfaceSettings {
                bearer_token: Arc::<str>::from("token-123"),
                hsts_enabled: false,
            },
        );

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/interrupt")
                    .header(AUTHORIZATION, "Bearer token-123")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"type":"interrupt","task_id":"missing-task"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.get("ok"), Some(&Value::Bool(false)));
        assert_eq!(
            payload.get("reason"),
            Some(&Value::String("task_not_found".to_string()))
        );
    }

    #[tokio::test]
    async fn test_approve_handler_returns_not_found_for_unknown_task() {
        let mut config = Config::default_for_tui();
        config.api.key = Some("token-123".to_string());
        let router = build_http_router(
            LocalApiState::new(config),
            HttpSurfaceSettings {
                bearer_token: Arc::<str>::from("token-123"),
                hsts_enabled: false,
            },
        );

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/approve")
                    .header(AUTHORIZATION, "Bearer token-123")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"type":"approve_capability","task_id":"missing-task","capability":"run_command","scope":"once"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.get("ok"), Some(&Value::Bool(false)));
        assert_eq!(
            payload.get("reason"),
            Some(&Value::String("task_not_found".to_string()))
        );
    }

    #[tokio::test]
    async fn test_approve_handler_returns_conflict_without_pending_approval() {
        let mut config = Config::default_for_tui();
        config.api.key = Some("token-123".to_string());
        let state = LocalApiState::new(config);
        let task_id = "task-approval-409".to_string();
        let (interrupt_tx, _interrupt_rx) = mpsc::unbounded_channel();
        let (envelope_tx, _envelope_rx) = mpsc::unbounded_channel();
        let quit = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(LocalApiTaskShared {
            normalizer: RuntimeEnvelopeNormalizer::new(task_id.clone()),
            envelope_tx,
            pending_approval: None,
            quit,
            turn_in_progress: false,
            interrupted: false,
        }));
        state.tasks.lock().await.insert(
            task_id.clone(),
            ActiveTask {
                interrupt_tx,
                shared,
            },
        );
        let router = build_http_router(
            state,
            HttpSurfaceSettings {
                bearer_token: Arc::<str>::from("token-123"),
                hsts_enabled: false,
            },
        );

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/approve")
                    .header(AUTHORIZATION, "Bearer token-123")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{"type":"approve_capability","task_id":"{task_id}","capability":"run_command","scope":"once"}}"#,
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.get("ok"), Some(&Value::Bool(false)));
        assert_eq!(
            payload.get("reason"),
            Some(&Value::String("no_pending_approval".to_string()))
        );
    }

    #[tokio::test]
    async fn test_turns_endpoint_rejects_schema_invalid_request() {
        let mut config = Config::default_for_tui();
        config.api.key = Some("token-123".to_string());
        let router = build_http_router(
            LocalApiState::new(config),
            HttpSurfaceSettings {
                bearer_token: Arc::<str>::from("token-123"),
                hsts_enabled: false,
            },
        );

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/turns")
                    .header(AUTHORIZATION, "Bearer token-123")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"type":"submit_input","task_id":"task-only"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn test_resolve_serve_config_rejects_non_loopback_without_tls() {
        let mut config = Config::default_for_tui();
        config.api.host = "192.168.1.20".to_string();
        config.api.key = Some("token-123".to_string());

        let error = resolve_serve_config(&config, None, None).unwrap_err();
        assert!(
            format!("{error:#}").contains("requires both api.tls_cert and api.tls_key"),
            "unexpected error: {error:#}"
        );
    }

    #[tokio::test]
    async fn test_local_api_mode_interrupt_emits_cancelled_turn_end() {
        let (envelope_tx, mut envelope_rx) = mpsc::unbounded_channel();
        let quit = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(LocalApiTaskShared {
            normalizer: RuntimeEnvelopeNormalizer::new("task-1"),
            envelope_tx,
            pending_approval: None,
            quit,
            turn_in_progress: false,
            interrupted: false,
        }));
        let mut mode = LocalApiMode::new(Arc::clone(&shared));
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation =
            ConversationManager::new(client, ToolOperator::new(std::env::temp_dir()));
        let (update_tx, _update_rx) = mpsc::unbounded_channel();
        let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());

        mode.on_user_input("review src/app.rs".to_string(), &mut ctx);
        let _ = envelope_rx.recv().await.unwrap();
        mode.on_interrupt(&mut ctx);
        mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

        let assistant: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        let terminal: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        assert!(matches!(
            assistant.event,
            RuntimeEvent::AssistantMessage { .. }
        ));
        assert!(matches!(
            terminal.event,
            RuntimeEvent::TurnEnd { ref status, .. } if status == "cancelled"
        ));
    }

    #[tokio::test]
    async fn test_local_api_mode_emits_approval_request_and_resolution() {
        let (envelope_tx, mut envelope_rx) = mpsc::unbounded_channel();
        let quit = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(LocalApiTaskShared {
            normalizer: RuntimeEnvelopeNormalizer::new("task-2"),
            envelope_tx,
            pending_approval: None,
            quit,
            turn_in_progress: false,
            interrupted: false,
        }));
        let mut mode = LocalApiMode::new(Arc::clone(&shared));
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation =
            ConversationManager::new(client, ToolOperator::new(std::env::temp_dir()));
        let (update_tx, _update_rx) = mpsc::unbounded_channel();
        let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
        let (response_tx, _response_rx) = tokio::sync::oneshot::channel();

        mode.on_user_input("review".to_string(), &mut ctx);
        let _ = envelope_rx.recv().await.unwrap();
        mode.on_model_update(
            UiUpdate::ToolApprovalRequest(crate::state::ToolApprovalRequest {
                tool_name: "run_command".to_string(),
                input_preview: "{}".to_string(),
                response_tx,
            }),
            &mut ctx,
        );

        let request: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        assert!(matches!(
            request.event,
            RuntimeEvent::ApprovalRequest { .. }
        ));

        {
            let mut shared = shared.lock().unwrap();
            let pending = shared.pending_approval.take().unwrap();
            let _ = pending.response_tx.send(true);
            let envelopes =
                shared
                    .normalizer
                    .normalize_runtime_request(&RuntimeRequest::ApproveCapability {
                        task_id: "task-2".to_string(),
                        capability: pending.capability,
                        scope: pending.scope,
                    });
            LocalApiMode::emit_envelopes(&mut shared, envelopes);
        }

        let resolved: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        assert!(matches!(
            resolved.event,
            RuntimeEvent::ApprovalResolved { approved: true, .. }
        ));
    }

    #[tokio::test]
    async fn test_local_api_mode_emits_stream_sequence_in_order() {
        let (envelope_tx, mut envelope_rx) = mpsc::unbounded_channel();
        let quit = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(LocalApiTaskShared {
            normalizer: RuntimeEnvelopeNormalizer::new("task-seq"),
            envelope_tx,
            pending_approval: None,
            quit,
            turn_in_progress: false,
            interrupted: false,
        }));
        let mut mode = LocalApiMode::new(Arc::clone(&shared));
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation =
            ConversationManager::new(client, ToolOperator::new(std::env::temp_dir()));
        let (update_tx, _update_rx) = mpsc::unbounded_channel();
        let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());

        mode.on_user_input("review src/local_api.rs".to_string(), &mut ctx);
        mode.on_model_update(UiUpdate::StreamDelta("working".to_string()), &mut ctx);
        mode.on_model_update(UiUpdate::TurnComplete, &mut ctx);

        let envelopes = [
            serde_json::from_str::<RuntimeEnvelope>(&envelope_rx.recv().await.unwrap()).unwrap(),
            serde_json::from_str::<RuntimeEnvelope>(&envelope_rx.recv().await.unwrap()).unwrap(),
            serde_json::from_str::<RuntimeEnvelope>(&envelope_rx.recv().await.unwrap()).unwrap(),
            serde_json::from_str::<RuntimeEnvelope>(&envelope_rx.recv().await.unwrap()).unwrap(),
        ];

        let seqs: Vec<u64> = envelopes.iter().map(|envelope| envelope.seq).collect();
        assert_eq!(seqs, vec![1, 2, 3, 4]);
        assert!(matches!(envelopes[0].event, RuntimeEvent::TurnStart { .. }));
        assert!(matches!(
            envelopes[1].event,
            RuntimeEvent::AssistantDelta { .. }
        ));
        assert!(matches!(
            envelopes[2].event,
            RuntimeEvent::AssistantMessage { .. }
        ));
        assert!(matches!(
            envelopes[3].event,
            RuntimeEvent::TurnEnd { ref status, .. } if status == "completed"
        ));
    }

    #[tokio::test]
    async fn test_local_api_mode_error_emits_failed_turn_end() {
        let (envelope_tx, mut envelope_rx) = mpsc::unbounded_channel();
        let quit = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(LocalApiTaskShared {
            normalizer: RuntimeEnvelopeNormalizer::new("task-error"),
            envelope_tx,
            pending_approval: None,
            quit,
            turn_in_progress: false,
            interrupted: false,
        }));
        let mut mode = LocalApiMode::new(Arc::clone(&shared));
        let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
        let conversation =
            ConversationManager::new(client, ToolOperator::new(std::env::temp_dir()));
        let (update_tx, _update_rx) = mpsc::unbounded_channel();
        let mut ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());

        mode.on_user_input("review src/local_api.rs".to_string(), &mut ctx);
        let _: RuntimeEnvelope = serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        mode.on_model_update(UiUpdate::Error("stream failed".to_string()), &mut ctx);

        let error: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();
        let terminal: RuntimeEnvelope =
            serde_json::from_str(&envelope_rx.recv().await.unwrap()).unwrap();

        assert!(matches!(
            error.event,
            RuntimeEvent::Error {
                ref code,
                ref message,
                recoverable: false,
            } if code == "runtime_error" && message == "stream failed"
        ));
        assert!(matches!(
            terminal.event,
            RuntimeEvent::TurnEnd { ref status, .. } if status == "failed"
        ));
    }

    #[test]
    fn test_resolve_serve_config_accepts_ipv4_loopback_aliases_without_tls() {
        let mut config = Config::default_for_tui();
        config.api.host = "127.42.0.7".to_string();
        config.api.key = Some("token-123".to_string());

        let resolved = resolve_serve_config(&config, None, None).unwrap();
        let http = resolved.http.expect("http surface should be present");
        assert_eq!(http.bind_addr, "127.42.0.7");
        assert!(http.tls.is_none());
    }

    #[test]
    fn test_resolve_serve_config_accepts_ipv6_loopback_without_tls() {
        let mut config = Config::default_for_tui();
        config.api.host = "::1".to_string();
        config.api.key = Some("token-123".to_string());

        let resolved = resolve_serve_config(&config, None, None).unwrap();
        let http = resolved.http.expect("http surface should be present");
        assert_eq!(http.bind_addr, "::1");
        assert!(http.tls.is_none());
    }

    #[test]
    fn test_resolve_serve_config_accepts_localhost_without_tls() {
        let mut config = Config::default_for_tui();
        config.api.host = "localhost".to_string();
        config.api.key = Some("token-123".to_string());

        let resolved = resolve_serve_config(&config, None, None).unwrap();
        let http = resolved.http.expect("http surface should be present");
        assert_eq!(http.bind_addr, "localhost");
        assert!(http.tls.is_none());
    }

    #[test]
    fn test_resolve_serve_config_rejects_vpn_trust_true() {
        let mut config = Config::default_for_tui();
        config.api.host = "127.0.0.1".to_string();
        config.api.key = Some("token-123".to_string());
        config.api.vpn_trust = true;

        let error = resolve_serve_config(&config, None, None).unwrap_err();
        assert!(
            format!("{error:#}").contains("api.vpn_trust must remain false"),
            "unexpected error: {error:#}"
        );
    }
}
