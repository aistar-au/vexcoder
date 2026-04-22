use super::{
    ApiConfigLayer, AutoMemoryConfigLayer, CompactionConfigLayer, ConfigLayer, SearchConfigLayer,
    UndoConfigLayer,
};
use crate::config::AutoMemoryConfig;


pub(super) fn apply_over(base: ConfigLayer, over: ConfigLayer) -> ConfigLayer {
    ConfigLayer {
        model_name: over.model_name.or(base.model_name),
        model_url: over.model_url.or(base.model_url),
        model_url_skip_tls_check: over
            .model_url_skip_tls_check
            .or(base.model_url_skip_tls_check),
        working_dir: over.working_dir.or(base.working_dir),
        sandbox: over.sandbox.or(base.sandbox),
        sandbox_profile: over.sandbox_profile.or(base.sandbox_profile),
        sandbox_require: over.sandbox_require.or(base.sandbox_require),
        model_backend: over.model_backend.or(base.model_backend),
        model_protocol: over.model_protocol.or(base.model_protocol),
        tool_call_mode: over.tool_call_mode.or(base.tool_call_mode),
        model_profile: over.model_profile.or(base.model_profile),
        max_project_instructions_tokens: over
            .max_project_instructions_tokens
            .or(base.max_project_instructions_tokens),
        max_memory_tokens: over.max_memory_tokens.or(base.max_memory_tokens),
        notes_path: over.notes_path.or(base.notes_path),
        api: apply_api_over(base.api, over.api),
        hooks: over.hooks.or(base.hooks),
        http_hooks: over.http_hooks.or(base.http_hooks),
        mcp_servers: over.mcp_servers.or(base.mcp_servers),
        compaction: apply_compaction_over(base.compaction, over.compaction),
        undo: apply_undo_over(base.undo, over.undo),
        search: apply_search_over(base.search, over.search),
        auto_memory: apply_auto_memory_over(base.auto_memory, over.auto_memory),
        api_client: over.api_client.or(base.api_client),
    }
}

fn apply_compaction_over(
    base: Option<CompactionConfigLayer>,
    over: Option<CompactionConfigLayer>,
) -> Option<CompactionConfigLayer> {
    match (base, over) {
        (None, None) => None,
        (Some(layer), None) | (None, Some(layer)) => Some(layer),
        (Some(base), Some(over)) => Some(CompactionConfigLayer {
            enabled: over.enabled.or(base.enabled),
            threshold_percent: over.threshold_percent.or(base.threshold_percent),
            keep_recent_turns: over.keep_recent_turns.or(base.keep_recent_turns),
            summary_max_tokens: over.summary_max_tokens.or(base.summary_max_tokens),
        }),
    }
}

fn apply_auto_memory_over(
    base: Option<AutoMemoryConfigLayer>,
    over: Option<AutoMemoryConfigLayer>,
) -> Option<AutoMemoryConfigLayer> {
    match (base, over) {
        (None, None) => None,
        (Some(layer), None) | (None, Some(layer)) => Some(layer),
        (Some(base), Some(over)) => Some(AutoMemoryConfigLayer {
            enabled: over.enabled.or(base.enabled),
            max_notes_per_turn: over.max_notes_per_turn.or(base.max_notes_per_turn),
        }),
    }
}

fn apply_undo_over(
    base: Option<UndoConfigLayer>,
    over: Option<UndoConfigLayer>,
) -> Option<UndoConfigLayer> {
    match (base, over) {
        (None, None) => None,
        (Some(layer), None) | (None, Some(layer)) => Some(layer),
        (Some(base), Some(over)) => Some(UndoConfigLayer {
            enabled: over.enabled.or(base.enabled),
            max_checkpoints: over.max_checkpoints.or(base.max_checkpoints),
        }),
    }
}

fn apply_search_over(
    base: Option<SearchConfigLayer>,
    over: Option<SearchConfigLayer>,
) -> Option<SearchConfigLayer> {
    match (base, over) {
        (None, None) => None,
        (Some(layer), None) | (None, Some(layer)) => Some(layer),
        (Some(base), Some(over)) => Some(SearchConfigLayer {
            enabled: over.enabled.or(base.enabled),
            auto_index: over.auto_index.or(base.auto_index),
            exclude: over.exclude.or(base.exclude),
            max_file_size: over.max_file_size.or(base.max_file_size),
        }),
    }
}

pub(super) fn resolve_auto_memory_config(layer: Option<AutoMemoryConfigLayer>) -> AutoMemoryConfig {
    let layer = layer.unwrap_or_default();
    let max_notes = layer.max_notes_per_turn.unwrap_or(3).clamp(1, 10);
    AutoMemoryConfig {
        enabled: layer.enabled.unwrap_or(false),
        max_notes_per_turn: max_notes,
    }
}

fn apply_api_over(
    base: Option<ApiConfigLayer>,
    over: Option<ApiConfigLayer>,
) -> Option<ApiConfigLayer> {
    match (base, over) {
        (None, None) => None,
        (Some(layer), None) | (None, Some(layer)) => Some(layer),
        (Some(base), Some(over)) => Some(ApiConfigLayer {
            transport: over.transport.or(base.transport),
            host: over.host.or(base.host),
            port: over.port.or(base.port),
            socket: over.socket.or(base.socket),
            key: over.key.or(base.key),
            tls_cert: over.tls_cert.or(base.tls_cert),
            tls_key: over.tls_key.or(base.tls_key),
            tls_ca_cert: over.tls_ca_cert.or(base.tls_ca_cert),
            tls_skip_verify: over.tls_skip_verify.or(base.tls_skip_verify),
            vpn_trust: over.vpn_trust.or(base.vpn_trust),
        }),
    }
}
