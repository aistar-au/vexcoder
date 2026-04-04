use super::*;
use crate::config::CompactionConfig;
use crate::runtime::backend::{ModelBackendKind, ModelProtocol, ToolCallMode};
use std::collections::BTreeSet;

#[test]
fn test_protocol_inference_defaults_to_messages_v1() {
    let protocol = infer_api_protocol("http://localhost:8000/v1/messages");
    assert_eq!(protocol, ApiProtocol::MessagesV1);
}

#[test]
fn test_protocol_inference_detects_chat_compat() {
    let protocol = infer_api_protocol("http://localhost:8000/v1/chat/completions");
    assert_eq!(protocol, ApiProtocol::ChatCompat);
}

#[test]
fn test_local_messages_endpoint_keeps_messages_v1_wire_protocol() {
    let config = crate::config::Config {
        model_token: None,
        model_name: "local/test-model".to_string(),
        model_url: "http://localhost:8000/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: std::path::PathBuf::from("."),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::TaggedFallback,
        model_profile: crate::types::ModelProfile::default_for_backend(
            ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: crate::config::UndoConfig::default(),
        search: crate::config::SearchConfig::default(),
        notes_path: None,
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
    };

    let client = ApiClient::new(&config).expect("client should build");
    assert_eq!(client.api_protocol(), ApiProtocol::MessagesV1);
    assert_eq!(client.request_url(), "http://localhost:8000/v1/messages");
    assert_eq!(client.protocol(), ModelProtocol::MessagesV1);
}

#[test]
fn test_local_bare_v1_endpoint_resolves_messages_v1_url() {
    let config = crate::config::Config {
        model_token: None,
        model_name: "local/test-model".to_string(),
        model_url: "http://localhost:8000/v1".to_string(),
        model_url_skip_tls_check: false,
        working_dir: std::path::PathBuf::from("."),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::TaggedFallback,
        model_profile: crate::types::ModelProfile::default_for_backend(
            ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: crate::config::UndoConfig::default(),
        search: crate::config::SearchConfig::default(),
        notes_path: None,
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
    };

    let client = ApiClient::new(&config).expect("client should build");
    assert_eq!(client.api_protocol(), ApiProtocol::MessagesV1);
    assert_eq!(client.request_url(), "http://localhost:8000/v1/messages");
    assert_eq!(client.protocol(), ModelProtocol::MessagesV1);
}

#[test]
fn test_local_bare_v1_endpoint_resolves_chat_compat_url() {
    let config = crate::config::Config {
        model_token: None,
        model_name: "local/test-model".to_string(),
        model_url: "http://localhost:8000/v1".to_string(),
        model_url_skip_tls_check: false,
        working_dir: std::path::PathBuf::from("."),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::ChatCompat,
        tool_call_mode: ToolCallMode::TaggedFallback,
        model_profile: crate::types::ModelProfile::default_for_backend(
            ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: crate::config::UndoConfig::default(),
        search: crate::config::SearchConfig::default(),
        notes_path: None,
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
    };

    let client = ApiClient::new(&config).expect("client should build");
    assert_eq!(client.api_protocol(), ApiProtocol::ChatCompat);
    assert_eq!(
        client.request_url(),
        "http://localhost:8000/v1/chat/completions"
    );
    assert_eq!(client.protocol(), ModelProtocol::ChatCompat);
}

#[test]
fn test_remote_messages_endpoint_preserves_messages_wire_protocol() {
    let config = crate::config::Config {
        model_token: Some("test-key".to_string()),
        model_name: "remote-test-model".to_string(),
        model_url: "https://model.example.internal/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: std::path::PathBuf::from("."),
        model_backend: ModelBackendKind::ApiServer,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::Structured,
        model_profile: crate::types::ModelProfile::default_for_backend(ModelBackendKind::ApiServer),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: crate::config::UndoConfig::default(),
        search: crate::config::SearchConfig::default(),
        notes_path: None,
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
    };

    let client = ApiClient::new(&config).expect("client should build");
    assert_eq!(client.api_protocol(), ApiProtocol::MessagesV1);
    assert_eq!(
        client.request_url(),
        "https://model.example.internal/v1/messages"
    );
}

#[test]
fn test_https_localhost_messages_endpoint_preserves_full_request_url() {
    let config = crate::config::Config {
        model_token: Some("test-key".to_string()),
        model_name: "remote-test-model".to_string(),
        model_url: "https://localhost:8443/v1/messages".to_string(),
        model_url_skip_tls_check: true,
        working_dir: std::path::PathBuf::from("."),
        model_backend: ModelBackendKind::ApiServer,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::Structured,
        model_profile: crate::types::ModelProfile::default_for_backend(ModelBackendKind::ApiServer),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: crate::config::UndoConfig::default(),
        search: crate::config::SearchConfig::default(),
        notes_path: None,
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
    };

    let client = ApiClient::new(&config).expect("client should build");
    assert_eq!(client.request_url(), "https://localhost:8443/v1/messages");
}

#[test]
fn test_local_plain_http_hint_suggests_plain_http_endpoint() {
    let hint = local_plain_http_hint("https://localhost:8000/v1/messages");
    assert_eq!(hint, " Try 'http://localhost:8000/v1/messages'.");
}

#[test]
fn test_chat_compat_url_adapter_from_messages_endpoint() {
    let adapted = adapt_to_chat_compat_url("http://localhost:8000/v1/messages");
    assert_eq!(adapted, "http://localhost:8000/v1/chat/completions");
}

#[test]
fn test_chat_compat_url_adapter_from_v1_base_endpoint() {
    let adapted = adapt_to_chat_compat_url("http://localhost:8000/v1");
    assert_eq!(adapted, "http://localhost:8000/v1/chat/completions");
}

#[test]
fn test_resolve_max_tokens_defaults_to_profile_budget() {
    // server_n_ctx=0 → ceiling=16384; default 4096 < 16384 → 4096
    let tokens = resolve_max_tokens(4096, 0);
    assert_eq!(tokens, 4096);
}

#[test]
fn test_resolve_max_tokens_uses_server_n_ctx() {
    // 75% of 65536 = 49152; default 4096 < 49152 → 4096
    let tokens = resolve_max_tokens(4096, 65536);
    assert_eq!(tokens, 4096);
}

#[test]
fn test_resolve_max_tokens_caps_at_seventy_five_percent_of_server_n_ctx() {
    // 75% of 65536 = 49152; default 60000 > 49152 → capped at 49152
    let tokens = resolve_max_tokens(60000, 65536);
    assert_eq!(tokens, 49152);
}

#[test]
fn test_resolve_max_tokens_unknown_server_caps_at_ceiling() {
    // server_n_ctx=0 → ceiling=16384; default 40000 > 16384 → capped at 16384
    let tokens = resolve_max_tokens(40000, 0);
    assert_eq!(tokens, 16384);
}

#[test]
fn test_tool_definitions_cover_execute_tool_dispatch_names() {
    let expected: BTreeSet<&str> = BTreeSet::from([
        "read_file",
        "write_file",
        "apply_patch",
        "edit_file",
        "rename_file",
        "list_files",
        "list_directory",
        "list_dir",
        "glob_files",
        "search_files",
        "search",
        "git_status",
        "git_diff",
        "git_log",
        "git_show",
        "git_add",
        "git_commit",
        "search_content",
        "find_files",
        "codebase_search",
    ]);

    let names: BTreeSet<String> = tool_definitions()
        .as_array()
        .expect("tool definitions must be an array")
        .iter()
        .filter_map(|tool: &Value| tool.get("name").and_then(|value: &Value| value.as_str()))
        .map(ToOwned::to_owned)
        .collect();

    let expected_owned: BTreeSet<String> = expected.iter().map(|s| s.to_string()).collect();
    assert_eq!(names, expected_owned);
}

#[test]
fn test_structured_tool_protocol_env_off_disables_protocol() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_STRUCTURED_TOOL_PROTOCOL", "off");
    let config = crate::config::Config {
        model_token: None,
        model_name: "mock-model".to_string(),
        model_url: "http://localhost:8000/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: std::path::PathBuf::from("."),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::TaggedFallback,
        model_profile: crate::types::ModelProfile::default_for_backend(
            ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: crate::config::UndoConfig::default(),
        search: crate::config::SearchConfig::default(),
        notes_path: None,
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
    };

    let client = ApiClient::new(&config).expect("client should build");
    assert!(!client.supports_structured_tool_protocol());
    std::env::remove_var("VEX_STRUCTURED_TOOL_PROTOCOL");
}

#[test]
fn test_structured_tool_protocol_defaults_off_for_local_endpoint() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::remove_var("VEX_STRUCTURED_TOOL_PROTOCOL");
    let config = crate::config::Config {
        model_token: None,
        model_name: "local/test-model".to_string(),
        model_url: "http://localhost:8000/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: std::path::PathBuf::from("."),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::TaggedFallback,
        model_profile: crate::types::ModelProfile::default_for_backend(
            ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: crate::config::UndoConfig::default(),
        search: crate::config::SearchConfig::default(),
        notes_path: None,
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
    };

    let client = ApiClient::new(&config).expect("client should build");
    assert!(!client.supports_structured_tool_protocol());
}

#[test]
fn test_structured_tool_protocol_defaults_on_for_remote_endpoint() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::remove_var("VEX_STRUCTURED_TOOL_PROTOCOL");
    let config = crate::config::Config {
        model_token: Some("test-key".to_string()),
        model_name: "mistral-7b-instruct".to_string(),
        model_url: "https://model.example.internal/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: std::path::PathBuf::from("."),
        model_backend: ModelBackendKind::ApiServer,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::Structured,
        model_profile: crate::types::ModelProfile::default_for_backend(ModelBackendKind::ApiServer),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: crate::config::UndoConfig::default(),
        search: crate::config::SearchConfig::default(),
        notes_path: None,
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
    };

    let client = ApiClient::new(&config).expect("client should build");
    assert!(client.supports_structured_tool_protocol());
}

#[test]
fn test_reserved_header_guard_blocks_auth_headers() {
    assert!(is_reserved_header("authorization"));
    assert!(is_reserved_header("Authorization"));
    assert!(is_reserved_header("x-api-key"));
    assert!(is_reserved_header("X-Api-Key"));
    assert!(is_reserved_header("content-length"));
    assert!(!is_reserved_header("x-custom-header"));
    assert!(!is_reserved_header("x-api-version"));
}

#[test]
fn test_chat_compat_tool_definitions_match_base_tool_names() {
    let base_names: BTreeSet<String> = tool_definitions()
        .as_array()
        .expect("tool definitions must be an array")
        .iter()
        .filter_map(|tool: &Value| tool.get("name").and_then(|value: &Value| value.as_str()))
        .map(ToOwned::to_owned)
        .collect();

    let chat_compat_names: BTreeSet<String> = tool_definitions_chat_compat_with_extra(&[])
        .as_array()
        .expect("chat-compat tool definitions must be an array")
        .iter()
        .filter_map(|tool| {
            tool.get("function")
                .and_then(|function| function.get("name"))
                .and_then(|name| name.as_str())
        })
        .map(ToOwned::to_owned)
        .collect();

    assert_eq!(chat_compat_names, base_names);
}

#[test]
fn test_system_prompt_includes_memory_notes() {
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
        vec![],
    )))
    .with_notes_content(Some("remember this".to_string()));
    let prompt = client.system_prompt();
    assert!(prompt.starts_with(BASE_SYSTEM_PROMPT));
    assert!(prompt.contains("<memory>\nremember this\n</memory>"));
}

#[test]
fn test_system_prompt_omits_blank_memory_notes() {
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
        vec![],
    )))
    .with_notes_content(Some("   \n".to_string()));
    assert_eq!(client.system_prompt(), BASE_SYSTEM_PROMPT);
}

#[test]
fn test_system_prompt_restricts_git_tool_capability_claims() {
    assert!(BASE_SYSTEM_PROMPT.contains("only list built-in git tools"));
    assert!(
        BASE_SYSTEM_PROMPT.contains("git_status, git_diff, git_log, git_show, git_add, git_commit")
    );
    assert!(BASE_SYSTEM_PROMPT.contains("Do not claim unsupported git tools"));
}

#[test]
fn test_system_prompt_allows_direct_answers_when_workspace_evidence_is_unneeded() {
    assert!(BASE_SYSTEM_PROMPT.contains("answer directly and do not call any tool"));
    assert!(BASE_SYSTEM_PROMPT
        .contains("Words like show, print, display, or list do not require a tool"));
}

#[test]
fn test_system_prompt_includes_large_file_edit_guidance() {
    assert!(BASE_SYSTEM_PROMPT.contains(
            "use write_file only for smaller full-file rewrites that stay under the write-file guard thresholds"
        ));
    assert!(BASE_SYSTEM_PROMPT
        .contains("For large files, prefer apply_patch or edit_file over write_file"));
    assert!(BASE_SYSTEM_PROMPT
        .contains("escalating to apply_patch when the change is too broad for edit_file"));
}

#[test]
fn test_write_file_tool_description_uses_guard_names_instead_of_hardcoded_numbers() {
    let definitions = tool_definitions();
    let description = definitions
        .as_array()
        .expect("tool definitions must be an array")
        .iter()
        .find(|entry: &&Value| {
            entry.get("name").and_then(|v: &Value| v.as_str()) == Some("write_file")
        })
        .and_then(|entry: &Value| entry.get("description"))
        .and_then(|value: &Value| value.as_str())
        .expect("write_file description must be present");

    assert!(description.contains("diff-preferred threshold"));
    assert!(description.contains("max line limit"));
    assert!(!description.contains("~200"));
    assert!(!description.contains("~500"));
}

#[test]
fn test_system_prompt_includes_history_condensing_awareness() {
    assert!(BASE_SYSTEM_PROMPT.contains("Tool results from earlier turns may be condensed"));
}

#[test]
fn test_with_project_instructions_none_uses_base_prompt() {
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
        vec![],
    )))
    .with_project_instructions(None);
    assert_eq!(client.effective_system_prompt(), BASE_SYSTEM_PROMPT);
}

#[test]
fn test_with_project_instructions_some_wraps_in_delimiters() {
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
        vec![],
    )))
    .with_project_instructions(Some("# no unwrap".to_string()));
    let prompt = client.effective_system_prompt();
    assert!(prompt.starts_with(BASE_SYSTEM_PROMPT));
    assert!(prompt.contains("[project instructions: start]"));
    assert!(prompt.contains("# no unwrap"));
    assert!(prompt.contains("[project instructions: end]"));
}

#[test]
fn test_system_prompt_includes_supplementary_prompt_when_set() {
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
        vec![],
    )));
    client.set_supplementary_system_prompt(Some("coding mode active".to_string()));

    let prompt = client.system_prompt();

    assert!(prompt.contains("[coding prompt: start]"));
    assert!(prompt.contains("coding mode active"));
    assert!(prompt.contains("[coding prompt: end]"));
}

#[test]
fn test_system_prompt_clears_supplementary_prompt() {
    let client = ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(
        vec![],
    )));
    client.set_supplementary_system_prompt(Some("coding mode active".to_string()));
    client.set_supplementary_system_prompt(None);

    assert_eq!(client.system_prompt(), BASE_SYSTEM_PROMPT);
}

#[test]
fn test_is_context_overflow_detects_token_exceeded() {
    assert!(is_context_overflow(
        "request (4291 tokens) exceeds the available context size (4096 tokens), try increasing it"
    ));
}

#[test]
fn test_is_context_overflow_detects_max_context_length() {
    assert!(is_context_overflow(
        "This model's maximum context length is 4096 tokens"
    ));
}

#[test]
fn test_is_context_overflow_negative() {
    assert!(!is_context_overflow("invalid model name"));
    assert!(!is_context_overflow("bad request"));
    assert!(!is_context_overflow(""));
}

#[test]
fn test_map_api_status_error_context_overflow_local() {
    let err = map_api_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        "request (4291 tokens) exceeds the available context size (4096 tokens)",
        "http://localhost:8000/v1/messages",
    );
    let msg = format!("{}", err);
    assert!(
        msg.contains("exceeded the server's context window"),
        "got: {msg}"
    );
    assert!(msg.contains("--ctx-size"), "got: {msg}");
    assert!(msg.contains("/compact"), "got: {msg}");
}

#[test]
fn test_map_api_status_error_generic_400_local() {
    let err = map_api_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        "invalid model name",
        "http://localhost:8000/v1/messages",
    );
    let msg = format!("{}", err);
    assert!(msg.contains("protocol"), "got: {msg}");
    assert!(msg.contains("MessagesV1"), "got: {msg}");
}

#[test]
fn test_map_api_status_error_remote_400() {
    let err = map_api_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        "bad request body",
        "https://api.example.com/v1/messages",
    );
    let msg = format!("{}", err);
    assert!(msg.contains("bad request body"), "got: {msg}");
    assert!(!msg.contains("--ctx-size"), "got: {msg}");
}

#[test]
fn test_map_api_status_error_server_500() {
    let err = map_api_status_error(
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        "internal server error: out of memory",
        "http://localhost:8000/v1/messages",
    );
    let msg = format!("{}", err);
    assert!(msg.contains("500"), "got: {msg}");
    assert!(msg.contains("out of memory"), "got: {msg}");
}

// ── URL adaptation: transposed /messages/v1 variant ──────────────────

#[test]
fn test_adapt_chat_compat_url_from_transposed_messages_v1() {
    let adapted = adapt_to_chat_compat_url("http://127.0.0.1:8000/messages/v1");
    assert_eq!(adapted, "http://127.0.0.1:8000/v1/chat/completions");
}

#[test]
fn test_adapt_messages_v1_url_from_transposed_messages_v1() {
    let adapted = adapt_to_messages_v1_url("http://127.0.0.1:8000/messages/v1");
    assert_eq!(adapted, "http://127.0.0.1:8000/v1/messages");
}

#[test]
fn test_adapt_chat_compat_url_from_transposed_messages_v1_with_trailing_slash() {
    let adapted = adapt_to_chat_compat_url("http://127.0.0.1:8000/messages/v1/");
    assert_eq!(adapted, "http://127.0.0.1:8000/v1/chat/completions");
}

#[test]
fn test_adapt_messages_v1_url_from_transposed_messages_v1_with_trailing_slash() {
    let adapted = adapt_to_messages_v1_url("http://127.0.0.1:8000/messages/v1/");
    assert_eq!(adapted, "http://127.0.0.1:8000/v1/messages");
}

// ── Protocol inference: transposed /messages/v1 ──────────────────────

#[test]
fn test_protocol_inference_transposed_messages_v1_is_messages() {
    let protocol = infer_api_protocol("http://127.0.0.1:8000/messages/v1");
    assert_eq!(protocol, ApiProtocol::MessagesV1);
}

#[test]
fn test_protocol_inference_standard_v1_messages_is_messages() {
    let protocol = infer_api_protocol("http://localhost:8000/v1/messages");
    assert_eq!(protocol, ApiProtocol::MessagesV1);
}

#[test]
fn test_protocol_inference_bare_v1_is_chat_compat() {
    let protocol = infer_api_protocol("http://localhost:8000/v1");
    assert_eq!(protocol, ApiProtocol::ChatCompat);
}

// ── Existing URL adaptations still correct ───────────────────────────

#[test]
fn test_adapt_messages_v1_url_from_chat_completions() {
    let adapted = adapt_to_messages_v1_url("http://localhost:8000/v1/chat/completions");
    assert_eq!(adapted, "http://localhost:8000/v1/messages");
}

#[test]
fn test_adapt_messages_v1_url_from_bare_v1() {
    let adapted = adapt_to_messages_v1_url("http://localhost:8000/v1");
    assert_eq!(adapted, "http://localhost:8000/v1/messages");
}

#[test]
fn test_adapt_messages_v1_url_already_correct() {
    let adapted = adapt_to_messages_v1_url("http://localhost:8000/v1/messages");
    assert_eq!(adapted, "http://localhost:8000/v1/messages");
}

#[test]
fn test_adapt_chat_compat_url_already_correct() {
    let adapted = adapt_to_chat_compat_url("http://localhost:8000/v1/chat/completions");
    assert_eq!(adapted, "http://localhost:8000/v1/chat/completions");
}

#[test]
fn test_apply_local_chat_compat_stream_flags_adds_progress_fields() {
    let mut payload = serde_json::Map::new();

    apply_local_chat_compat_stream_flags(&mut payload);

    assert_eq!(payload.get("return_progress"), Some(&json!(true)));
    assert_eq!(payload.get("timings_per_token"), Some(&json!(true)));
    assert_eq!(
        payload.get("cache_prompt"),
        Some(&json!(true)),
        "cache_prompt must be enabled for local servers to allow batch prompt evaluation"
    );
}

// ── Connected-server smoke test (optional; skips if server unreachable) ───

#[tokio::test]
async fn test_live_server_chat_completions_reachable() {
    let url = std::env::var("VEX_TEST_LIVE_SERVER_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8000".to_string());
    let endpoint = format!("{}/v1/chat/completions", url.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("http client");

    let payload = serde_json::json!({
        "model": "test",
        "max_tokens": 4,
        "temperature": 0.0,
        "stream": false,
        "messages": [{"role": "user", "content": "Reply OK"}]
    });

    match client.post(&endpoint).json(&payload).send().await {
        Ok(resp) => {
            // Server is reachable — verify it doesn't 404 on the native endpoint.
            assert_ne!(
                resp.status().as_u16(),
                404,
                "connected server returned 404 on native chat/completions endpoint"
            );
        }
        Err(_) => {
            // Server not available — skip gracefully.
            eprintln!(
                "SKIP: server at {} not reachable, skipping connectivity check",
                endpoint
            );
        }
    }
}

#[tokio::test]
async fn test_live_server_messages_v1_reachable() {
    let url = std::env::var("VEX_TEST_LIVE_SERVER_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8000".to_string());
    let endpoint = format!("{}/v1/messages", url.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("http client");

    let payload = serde_json::json!({
        "model": "test",
        "max_tokens": 4,
        "stream": false,
        "system": "Reply OK",
        "messages": [{"role": "user", "content": "OK"}]
    });

    match client.post(&endpoint).json(&payload).send().await {
        Ok(resp) => {
            assert_ne!(
                resp.status().as_u16(),
                404,
                "connected server returned 404 on messages/v1 endpoint"
            );
        }
        Err(_) => {
            eprintln!(
                "SKIP: server at {} not reachable, skipping connectivity check",
                endpoint
            );
        }
    }
}

// ── Protocol conversion boundary regression tests ────────────────────

#[test]
fn test_native_protocol_overrides_configured_protocol() {
    // When server discovery detects native ChatCompat, the client must
    // use ChatCompat even if the user configured MessagesV1 — this is
    // the core boundary that prevents server-side conversion.
    let client = ApiClient {
        http: reqwest::Client::new(),
        api_key: None,
        model: Arc::new(RwLock::new("test".to_string())),
        supplementary_system_prompt: Arc::new(RwLock::new(None)),
        api_url: "http://localhost:8000/v1/messages".to_string(),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::Structured,
        model_headers: reqwest::header::HeaderMap::new(),
        temperature: 0.3,
        top_p: 1.0,
        max_tokens: 4096,
        stop_sequences: Vec::new(),
        reasoning_budget: 0,
        project_instructions: None,
        notes_content: None,
        extra_tool_definitions: Vec::new(),
        server_info: Arc::new(RwLock::new(Some(ServerInfo {
            n_ctx: 65536,
            n_batch: 2048,
            model: "test".to_string(),
            native_protocol: Some(ModelProtocol::ChatCompat),
        }))),
        #[cfg(test)]
        mock_stream_producer: None,
    };

    assert_eq!(
        client.api_protocol(),
        ApiProtocol::ChatCompat,
        "client must use native ChatCompat when server reports it, \
             even when user configured MessagesV1"
    );
}

#[test]
fn test_no_native_protocol_falls_back_to_configured() {
    // When server discovery did not detect a native protocol, the
    // client must respect the user-configured protocol.
    let client = ApiClient {
        http: reqwest::Client::new(),
        api_key: None,
        model: Arc::new(RwLock::new("test".to_string())),
        supplementary_system_prompt: Arc::new(RwLock::new(None)),
        api_url: "http://localhost:8000/v1/messages".to_string(),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::Structured,
        model_headers: reqwest::header::HeaderMap::new(),
        temperature: 0.3,
        top_p: 1.0,
        max_tokens: 4096,
        stop_sequences: Vec::new(),
        reasoning_budget: 0,
        project_instructions: None,
        notes_content: None,
        extra_tool_definitions: Vec::new(),
        server_info: Arc::new(RwLock::new(Some(ServerInfo {
            n_ctx: 65536,
            n_batch: 2048,
            model: "test".to_string(),
            native_protocol: None,
        }))),
        #[cfg(test)]
        mock_stream_producer: None,
    };

    assert_eq!(
        client.api_protocol(),
        ApiProtocol::MessagesV1,
        "without native_protocol, client must fall back to configured MessagesV1"
    );
}

#[test]
fn test_server_info_native_protocol_field_default() {
    let info = ServerInfo::default();
    assert!(
        info.native_protocol.is_none(),
        "ServerInfo::default() must have native_protocol = None"
    );
}

#[test]
fn test_system_prompt_forbids_shell_utilities() {
    let prompt = BASE_SYSTEM_PROMPT;
    assert!(
        prompt.contains("run_shell_command"),
        "system prompt must explicitly forbid run_shell_command"
    );
    assert!(
        prompt.contains("Shell utilities"),
        "system prompt must mention shell utilities are unavailable"
    );
    assert!(
        !prompt.contains("e.g. do not call run_shell_command"),
        "system prompt must use the stronger shell-utility prohibition"
    );
}
