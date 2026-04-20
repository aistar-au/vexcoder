use anyhow::{Context as AnyhowContext, Result};
use opentelemetry::baggage::BaggageExt;
use opentelemetry::trace::TraceContextExt;
use opentelemetry::{Context, KeyValue, global, propagation::TextMapCompositePropagator};
use opentelemetry_sdk::propagation::{BaggagePropagator, TraceContextPropagator};
use std::fs::OpenOptions;
use std::path::PathBuf;
use tracing::Span;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub const REQUEST_ID_HEADER: &str = "x-request-id";
pub const DISPLAY_INTERNAL_TELEMETRY_FLAG: &str = "--display-internal-telemetry";
pub const TELEMETRY_FORMAT_ENV: &str = "VEX_TELEMETRY_FORMAT";
pub const TELEMETRY_PATH_ENV: &str = "VEX_INTERNAL_TELEMETRY_PATH";
pub const TELEMETRY_ENVIRONMENT_ENV: &str = "VEX_TELEMETRY_ENVIRONMENT";
pub const TELEMETRY_TENANT_ENV: &str = "VEX_TELEMETRY_TENANT_ID";

const DEFAULT_TELEMETRY_FILTER: &str =
    "vex=debug,vexcoder=debug,tower_http=info,axum_tracing_opentelemetry=info";
const DEFAULT_TELEMETRY_FILENAME: &str = "internal-telemetry.log";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CliTelemetryOptions {
    pub display_internal_telemetry: bool,
    pub telemetry_json: bool,
}

pub struct TelemetryGuards {
    _writer_guard: WorkerGuard,
}

pub fn new_request_id() -> String {
    uuid::Uuid::new_v4().as_hyphenated().to_string()
}

pub fn init_cli_observability(options: CliTelemetryOptions) -> Result<Option<TelemetryGuards>> {
    if !telemetry_requested(options) {
        return Ok(None);
    }

    install_w3c_propagation();

    let env_filter = resolve_env_filter(options)?;
    let telemetry_format = resolve_telemetry_format(options)?;
    let (writer, writer_guard) = build_telemetry_writer(options.display_internal_telemetry)?;
    let tracer = global::tracer("vexcoder.internal");
    let otel_layer = tracing_opentelemetry::layer()
        .with_context_activation(true)
        .with_tracer(tracer);
    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(otel_layer);

    match telemetry_format {
        TelemetryFormat::Text => registry
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(writer)
                    .with_target(true)
                    .with_thread_ids(true)
                    .with_thread_names(true)
                    .with_span_events(FmtSpan::CLOSE),
            )
            .try_init()
            .map_err(|error| {
                anyhow::anyhow!("failed to initialize text telemetry subscriber: {error}")
            })?,
        TelemetryFormat::Json => registry
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(writer)
                    .with_target(true)
                    .with_thread_ids(true)
                    .with_thread_names(true)
                    .with_span_events(FmtSpan::CLOSE)
                    .json()
                    .with_current_span(true)
                    .with_span_list(true),
            )
            .try_init()
            .map_err(|error| {
                anyhow::anyhow!("failed to initialize JSON telemetry subscriber: {error}")
            })?,
    }

    Ok(Some(TelemetryGuards {
        _writer_guard: writer_guard,
    }))
}

pub fn attach_standard_baggage(
    span: &Span,
    surface: &str,
    workflow: &str,
    request_id: Option<&str>,
    task_id: Option<&str>,
) {
    let mut baggage = vec![
        KeyValue::new("vex.surface", surface.to_string()),
        KeyValue::new("vex.workflow", workflow.to_string()),
        KeyValue::new("deployment.environment.name", telemetry_environment_label()),
    ];

    if let Some(request_id) = request_id {
        baggage.push(KeyValue::new("request.id", request_id.to_string()));
    }
    if let Some(task_id) = task_id {
        baggage.push(KeyValue::new("task.id", task_id.to_string()));
    }
    if let Some(tenant_id) = telemetry_tenant_id() {
        baggage.push(KeyValue::new("tenant.id", tenant_id));
    }

    let parent_context = Context::current().with_baggage(baggage);
    let _ = span.set_parent(parent_context);
    record_trace_context_fields(span);
}

pub fn set_span_attributes(span: &Span, attributes: impl IntoIterator<Item = KeyValue>) {
    for attribute in attributes {
        span.set_attribute(attribute.key, attribute.value);
    }
    record_trace_context_fields(span);
}

pub fn record_trace_context_fields(span: &Span) {
    let span_context = span.context().span().span_context().clone();
    if !span_context.is_valid() {
        return;
    }

    span.record("trace_id", tracing::field::display(span_context.trace_id()));
    span.record(
        "otel_span_id",
        tracing::field::display(span_context.span_id()),
    );
}

pub fn telemetry_environment_label() -> String {
    std::env::var(TELEMETRY_ENVIRONMENT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "local".to_string())
}

fn telemetry_requested(options: CliTelemetryOptions) -> bool {
    options.display_internal_telemetry
        || std::env::var_os("RUST_LOG").is_some()
        || std::env::var_os(TELEMETRY_PATH_ENV).is_some()
        || std::env::var_os(TELEMETRY_FORMAT_ENV).is_some()
}

fn install_w3c_propagation() {
    global::set_text_map_propagator(TextMapCompositePropagator::new(vec![
        Box::new(TraceContextPropagator::new()),
        Box::new(BaggagePropagator::new()),
    ]));
}

fn resolve_env_filter(options: CliTelemetryOptions) -> Result<EnvFilter> {
    if std::env::var_os("RUST_LOG").is_some() {
        return EnvFilter::try_from_default_env()
            .map_err(|error| anyhow::anyhow!("invalid RUST_LOG filter: {error}"));
    }

    let fallback = if options.display_internal_telemetry {
        DEFAULT_TELEMETRY_FILTER
    } else {
        "info"
    };
    EnvFilter::try_new(fallback)
        .map_err(|error| anyhow::anyhow!("invalid fallback telemetry filter '{fallback}': {error}"))
}

fn resolve_telemetry_format(options: CliTelemetryOptions) -> Result<TelemetryFormat> {
    if options.telemetry_json {
        return Ok(TelemetryFormat::Json);
    }

    match std::env::var(TELEMETRY_FORMAT_ENV) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "json" | "ndjson" | "jsonl" => Ok(TelemetryFormat::Json),
            "text" | "pretty" => Ok(TelemetryFormat::Text),
            other => Err(anyhow::anyhow!(
                "invalid {} value '{}': expected text or json",
                TELEMETRY_FORMAT_ENV,
                other
            )),
        },
        Err(_) => Ok(TelemetryFormat::Text),
    }
}

fn build_telemetry_writer(display_internal_telemetry: bool) -> Result<(NonBlocking, WorkerGuard)> {
    if display_internal_telemetry {
        return Ok(tracing_appender::non_blocking(std::io::stderr()));
    }

    let path = telemetry_log_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create telemetry log directory '{}'",
                parent.display()
            )
        })?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open telemetry log file '{}'", path.display()))?;
    Ok(tracing_appender::non_blocking(file))
}

fn telemetry_log_path() -> PathBuf {
    std::env::var(TELEMETRY_PATH_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_telemetry_log_path)
}

fn default_telemetry_log_path() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("vex")
        .join("logs")
        .join(DEFAULT_TELEMETRY_FILENAME)
}

fn telemetry_tenant_id() -> Option<String> {
    std::env::var(TELEMETRY_TENANT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_telemetry_log_path_uses_log_filename() {
        assert!(default_telemetry_log_path().ends_with(DEFAULT_TELEMETRY_FILENAME));
    }

    #[test]
    fn test_resolve_telemetry_format_accepts_json_aliases() {
        let env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        let _guard = crate::test_support::EnvRestore::capture(&env_lock, TELEMETRY_FORMAT_ENV);

        crate::test_support::test_set_var(&env_lock, TELEMETRY_FORMAT_ENV, "jsonl");
        assert_eq!(
            resolve_telemetry_format(CliTelemetryOptions::default()).unwrap(),
            TelemetryFormat::Json
        );
    }
}
