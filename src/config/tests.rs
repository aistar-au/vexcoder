use super::{Config, ModelBackendKind};
use std::path::PathBuf;

use crate::types::ModelProfile;

struct EnvRestore {
    key: &'static str,
    value: Option<String>,
}

impl EnvRestore {
    fn capture(key: &'static str) -> Self {
        Self {
            key,
            value: std::env::var(key).ok(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        match &self.value {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

#[test]
fn test_config_rejects_non_loopback_http_model_url() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _url = EnvRestore::capture("VEX_MODEL_URL");
    let _name = EnvRestore::capture("VEX_MODEL_NAME");
    let _token = EnvRestore::capture("VEX_MODEL_TOKEN");

    std::env::set_var("VEX_MODEL_URL", "http://api.example.internal/v1/messages");
    std::env::set_var("VEX_MODEL_NAME", "remote-model");
    std::env::set_var("VEX_MODEL_TOKEN", "token");

    let cfg = Config::load().expect("load failed");
    let error = cfg
        .validate()
        .expect_err("non-loopback http must be rejected");
    assert!(error.to_string().contains("https://"), "{error:#}");
}

#[test]
fn test_config_allows_loopback_http_model_url() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _url = EnvRestore::capture("VEX_MODEL_URL");
    let _name = EnvRestore::capture("VEX_MODEL_NAME");
    let _token = EnvRestore::capture("VEX_MODEL_TOKEN");

    std::env::set_var("VEX_MODEL_URL", "http://127.0.0.1:8080/v1/messages");
    std::env::set_var("VEX_MODEL_NAME", "local-model");
    std::env::remove_var("VEX_MODEL_TOKEN");

    let cfg = Config::load().expect("load failed");
    assert!(cfg.validate().is_ok(), "loopback http must remain valid");
}

#[test]
fn test_config_allows_private_network_http_model_url() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _url = EnvRestore::capture("VEX_MODEL_URL");
    let _name = EnvRestore::capture("VEX_MODEL_NAME");
    let _token = EnvRestore::capture("VEX_MODEL_TOKEN");

    // LAN-reachable model server on a private RFC 1918 address
    std::env::set_var("VEX_MODEL_URL", "http://192.168.1.100:11434/v1");
    std::env::set_var("VEX_MODEL_NAME", "local-model");
    std::env::remove_var("VEX_MODEL_TOKEN");

    let cfg = Config::load().expect("load failed");
    assert!(
        cfg.validate().is_ok(),
        "private-network http must remain valid"
    );
}

#[test]
fn test_config_loads_vex_model_name_without_legacy_prefix() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
    std::env::set_var("VEX_MODEL_NAME", "local-model-70b");
    std::env::remove_var("VEX_MODEL_TOKEN");

    let cfg = Config::load().expect("load failed");
    assert!(
        cfg.validate().is_ok(),
        "neutral model name must pass validation"
    );
    std::env::remove_var("VEX_MODEL_URL");
    std::env::remove_var("VEX_MODEL_NAME");
}

#[test]
fn test_model_backend_kind_parses_from_env_var() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_MODEL_BACKEND", "local-runtime");
    std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
    std::env::set_var("VEX_MODEL_NAME", "local-model");
    let cfg = Config::load().expect("load failed");
    assert!(cfg.validate().is_ok());
    assert_eq!(cfg.model_backend, ModelBackendKind::LocalRuntime);
    std::env::remove_var("VEX_MODEL_BACKEND");
    std::env::remove_var("VEX_MODEL_URL");
    std::env::remove_var("VEX_MODEL_NAME");
}

#[test]
fn test_invalid_model_protocol_env_var_is_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
    std::env::set_var("VEX_MODEL_NAME", "mock-model");
    std::env::set_var("VEX_MODEL_PROTOCOL", "legacy-value");

    assert!(Config::load().is_err());

    std::env::remove_var("VEX_MODEL_URL");
    std::env::remove_var("VEX_MODEL_NAME");
    std::env::remove_var("VEX_MODEL_PROTOCOL");
}

#[test]
fn test_repo_local_api_key_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let cwd = repo_root.join("nested/project");

    std::fs::create_dir_all(repo_root.join(".vex")).unwrap();
    std::fs::create_dir_all(repo_root.join(".git")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        repo_root.join(".vex/config.toml"),
        "[api]\nkey = \"literal-secret\"\n",
    )
    .unwrap();

    let error = Config::load_for_tests(&cwd, None, None).unwrap_err();
    assert!(
        format!("{error:#}").contains("api.key"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn test_repo_local_model_url_skip_tls_check_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let cwd = repo_root.join("nested/project");

    std::fs::create_dir_all(repo_root.join(".vex")).unwrap();
    std::fs::create_dir_all(repo_root.join(".git")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        repo_root.join(".vex/config.toml"),
        "model_url_skip_tls_check = true\n",
    )
    .unwrap();

    let error = Config::load_for_tests(&cwd, None, None).unwrap_err();
    assert!(
        format!("{error:#}").contains("model_url_skip_tls_check"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn test_user_api_key_env_reference_resolves() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _api_key = EnvRestore::capture("VEX_API_KEY");
    std::env::set_var("VEX_API_KEY", "resolved-secret");

    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    let user_cfg = temp.path().join("user.toml");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(&user_cfg, "[api]\nkey = \"${VEX_API_KEY}\"\n").unwrap();

    let cfg = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap();
    assert_eq!(cfg.api.key.as_deref(), Some("resolved-secret"));
}

#[test]
fn test_api_tls_skip_verify_true_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let cwd = repo_root.join("nested/project");

    std::fs::create_dir_all(repo_root.join(".vex")).unwrap();
    std::fs::create_dir_all(repo_root.join(".git")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        repo_root.join(".vex/config.toml"),
        "[api]\ntls_skip_verify = true\n",
    )
    .unwrap();

    let error = Config::load_for_tests(&cwd, None, None).unwrap_err();
    assert!(
        format!("{error:#}").contains("tls_skip_verify"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn test_api_vpn_trust_true_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let cwd = repo_root.join("nested/project");

    std::fs::create_dir_all(repo_root.join(".vex")).unwrap();
    std::fs::create_dir_all(repo_root.join(".git")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        repo_root.join(".vex/config.toml"),
        "[api]\nvpn_trust = true\n",
    )
    .unwrap();

    let error = Config::load_for_tests(&cwd, None, None).unwrap_err();
    assert!(
        format!("{error:#}").contains("vpn_trust"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn test_invalid_model_backend_error_lists_remote_alias() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_MODEL_BACKEND", "legacy-value");

    let err = super::read_env_layer().unwrap_err();
    let msg = format!("{err:#}");

    assert!(
        msg.contains("remote"),
        "expected remote alias in error: {msg}"
    );
    std::env::remove_var("VEX_MODEL_BACKEND");
}

#[test]
fn test_invalid_tool_call_mode_error_lists_fallback_alias() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_TOOL_CALL_MODE", "legacy-value");

    let err = super::read_env_layer().unwrap_err();
    let msg = format!("{err:#}");

    assert!(
        msg.contains("fallback"),
        "expected fallback alias in error: {msg}"
    );
    std::env::remove_var("VEX_TOOL_CALL_MODE");
}

#[test]
fn test_user_config_path_prefers_xdg_config_home() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _home = EnvRestore::capture("HOME");
    let _xdg = EnvRestore::capture("XDG_CONFIG_HOME");
    let temp = tempfile::tempdir().unwrap();
    let xdg_root = temp.path().join("xdg-root");
    let legacy_home = temp.path().join("home");
    let xdg_path = xdg_root.join("vex").join("config.toml");
    let legacy_path = legacy_home.join(".vex").join("config.toml");

    std::fs::create_dir_all(xdg_path.parent().unwrap()).unwrap();
    std::fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
    std::fs::write(&xdg_path, "model_name = \"xdg\"\n").unwrap();
    std::fs::write(&legacy_path, "model_name = \"legacy\"\n").unwrap();
    std::env::set_var("HOME", &legacy_home);
    std::env::set_var("XDG_CONFIG_HOME", &xdg_root);

    assert_eq!(super::user_config_path(), Some(xdg_path));
}

#[test]
fn test_user_config_path_falls_back_to_legacy_home_config() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _home = EnvRestore::capture("HOME");
    let _xdg = EnvRestore::capture("XDG_CONFIG_HOME");
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let legacy_path = home.join(".vex").join("config.toml");

    std::fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
    std::fs::write(&legacy_path, "model_name = \"legacy\"\n").unwrap();
    std::env::set_var("HOME", &home);
    std::env::remove_var("XDG_CONFIG_HOME");

    assert_eq!(super::user_config_path(), Some(legacy_path));
}

#[test]
fn test_doctor_snapshot_matches_runtime_working_dir_resolution() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _workdir = EnvRestore::capture("VEX_WORKDIR");
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("VEX_WORKDIR", "~/doctor-workdir");

    let config = Config::load_for_tests(temp.path(), None, None).unwrap();
    let snapshot = super::doctor_snapshot(temp.path()).unwrap();

    assert_eq!(config.working_dir, PathBuf::from("~/doctor-workdir"));
    assert_eq!(snapshot.working_dir, config.working_dir);
    std::env::remove_var("VEX_WORKDIR");
}

#[test]
fn test_parse_model_headers_json_valid() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var(
        "VEX_MODEL_HEADERS_JSON",
        r#"{"x-custom-header": "value1", "x-other": "value2"}"#,
    );
    let headers = super::parse_model_headers_json().unwrap();
    assert_eq!(headers.len(), 2);
    std::env::remove_var("VEX_MODEL_HEADERS_JSON");
}

#[test]
fn test_parse_model_headers_json_invalid_name_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_MODEL_HEADERS_JSON", r#"{"invalid header!": "v"}"#);
    assert!(super::parse_model_headers_json().is_err());
    std::env::remove_var("VEX_MODEL_HEADERS_JSON");
}

#[test]
fn test_parse_model_headers_json_non_string_value_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_MODEL_HEADERS_JSON", r#"{"x-count": 42}"#);
    assert!(super::parse_model_headers_json().is_err());
    std::env::remove_var("VEX_MODEL_HEADERS_JSON");
}

#[test]
fn test_parse_model_headers_json_empty_env_returns_empty_map() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::remove_var("VEX_MODEL_HEADERS_JSON");
    let headers = super::parse_model_headers_json().unwrap();
    assert!(headers.is_empty());
}

#[test]
fn test_max_project_instructions_tokens_env_sets_field() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS", "2048");
    std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
    std::env::set_var("VEX_MODEL_NAME", "test-model");
    let cfg = Config::load().expect("load failed");
    assert_eq!(cfg.max_project_instructions_tokens, 2048);
    std::env::remove_var("VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS");
    std::env::remove_var("VEX_MODEL_URL");
    std::env::remove_var("VEX_MODEL_NAME");
}

#[test]
fn test_max_project_instructions_tokens_defaults_to_4096() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::remove_var("VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS");
    std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
    std::env::set_var("VEX_MODEL_NAME", "test-model");
    let cfg = Config::load().expect("load failed");
    assert_eq!(cfg.max_project_instructions_tokens, 4096);
    std::env::remove_var("VEX_MODEL_URL");
    std::env::remove_var("VEX_MODEL_NAME");
}

#[test]
fn test_max_project_instructions_tokens_zero_uses_default() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS", "0");
    std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
    std::env::set_var("VEX_MODEL_NAME", "test-model");
    let cfg = Config::load().expect("load failed");
    assert_eq!(cfg.max_project_instructions_tokens, 4096);
    std::env::remove_var("VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS");
    std::env::remove_var("VEX_MODEL_URL");
    std::env::remove_var("VEX_MODEL_NAME");
}

#[test]
fn test_max_memory_tokens_env_sets_field() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_MAX_MEMORY_TOKENS", "1024");
    std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
    std::env::set_var("VEX_MODEL_NAME", "test-model");
    let cfg = Config::load().expect("load failed");
    assert_eq!(cfg.max_memory_tokens, 1024);
    std::env::remove_var("VEX_MAX_MEMORY_TOKENS");
    std::env::remove_var("VEX_MODEL_URL");
    std::env::remove_var("VEX_MODEL_NAME");
}

#[test]
fn test_max_memory_tokens_defaults_to_2048() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::remove_var("VEX_MAX_MEMORY_TOKENS");
    std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
    std::env::set_var("VEX_MODEL_NAME", "test-model");
    let cfg = Config::load().expect("load failed");
    assert_eq!(cfg.max_memory_tokens, 2048);
    std::env::remove_var("VEX_MODEL_URL");
    std::env::remove_var("VEX_MODEL_NAME");
}

#[test]
fn test_default_for_tui_returns_local_defaults() {
    let cfg = Config::default_for_tui();
    assert_eq!(cfg.model_name, "local/default");
    assert_eq!(cfg.model_backend, ModelBackendKind::LocalRuntime);
    assert_eq!(
        cfg.model_profile,
        ModelProfile::default_for_backend(ModelBackendKind::LocalRuntime)
    );
    assert!(!cfg.model_url_skip_tls_check);
    assert!(cfg.model_token.is_none());
    assert!(cfg.hooks.is_empty());
}

#[test]
fn test_model_url_skip_tls_check_warns() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _skip = EnvRestore::capture("VEX_MODEL_URL_SKIP_TLS_CHECK");
    let _url = EnvRestore::capture("VEX_MODEL_URL");
    let _name = EnvRestore::capture("VEX_MODEL_NAME");
    std::env::set_var("VEX_MODEL_URL_SKIP_TLS_CHECK", "true");
    std::env::set_var("VEX_MODEL_URL", "https://localhost:8443/v1/messages");
    std::env::set_var("VEX_MODEL_NAME", "test-model");

    let cfg = Config::load().expect("load failed");
    assert!(cfg.model_url_skip_tls_check);
    assert!(cfg.should_warn_about_model_tls_skip_check());
}

#[test]
fn test_interactive_selection_preserves_non_default_runtime_shape() {
    let mut cfg = Config::default_for_tui();
    cfg.model_url = "http://localhost:8000/v1/messages".to_string();
    cfg.model_name = "test-model".to_string();
    cfg.model_backend = ModelBackendKind::ApiServer;
    cfg.model_protocol = crate::runtime::ModelProtocol::ChatCompat;
    cfg.tool_call_mode = crate::runtime::ToolCallMode::Structured;

    cfg.apply_interactive_model_selection(
        "http://localhost:9000/v1/messages".to_string(),
        Some("test-model-2".to_string()),
    );

    assert_eq!(cfg.model_url, "http://localhost:9000/v1/messages");
    assert_eq!(cfg.model_name, "test-model-2");
    assert_eq!(cfg.model_backend, ModelBackendKind::ApiServer);
    assert_eq!(
        cfg.model_protocol,
        crate::runtime::ModelProtocol::ChatCompat
    );
    assert_eq!(cfg.tool_call_mode, crate::runtime::ToolCallMode::Structured);
}

#[test]
fn test_max_memory_tokens_zero_uses_default() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_MAX_MEMORY_TOKENS", "0");
    std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
    std::env::set_var("VEX_MODEL_NAME", "test-model");
    let cfg = Config::load().expect("load failed");
    assert_eq!(cfg.max_memory_tokens, 2048);
    std::env::remove_var("VEX_MAX_MEMORY_TOKENS");
    std::env::remove_var("VEX_MODEL_URL");
    std::env::remove_var("VEX_MODEL_NAME");
}

#[test]
fn test_model_profile_loaded_from_layered_config() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let cwd = repo_root.join("nested/project");
    let user_cfg = temp.path().join("user.toml");

    std::fs::create_dir_all(repo_root.join("models")).unwrap();
    std::fs::create_dir_all(repo_root.join(".git")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        repo_root.join("models/api-structured.toml"),
        std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models/api-structured.toml"),
        )
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        &user_cfg,
        "model_profile = \"models/api-structured.toml\"\n",
    )
    .unwrap();

    let cfg = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap();

    assert_eq!(cfg.model_profile.name, "api-structured");
    assert_eq!(cfg.tool_call_mode, cfg.model_profile.tool_call_mode());
}

#[test]
fn test_migrate_maps_legacy_messages_protocol() {
    let out = super::migrate_config_from_env(&[(
        "VEX_API_PROTOCOL",
        super::legacy_messages_protocol_value(),
    )]);
    assert!(out.contains("model_protocol = \"messages-v1\""), "{out}");
}

#[test]
fn test_migrate_maps_legacy_chat_protocol() {
    let out = super::migrate_config_from_env(&[(
        "VEX_API_PROTOCOL",
        super::legacy_chat_protocol_value(),
    )]);
    assert!(out.contains("model_protocol = \"chat-compat\""), "{out}");
}

#[test]
fn test_migrate_maps_structured_tool_protocol_on() {
    let out = super::migrate_config_from_env(&[("VEX_STRUCTURED_TOOL_PROTOCOL", "on")]);
    assert!(out.contains("tool_call_mode = \"structured\""), "{out}");
}

#[test]
fn test_migrate_maps_structured_tool_protocol_off() {
    let out = super::migrate_config_from_env(&[("VEX_STRUCTURED_TOOL_PROTOCOL", "off")]);
    assert!(
        out.contains("tool_call_mode = \"tagged-fallback\""),
        "{out}"
    );
}

#[test]
fn test_migrate_strips_v1_messages_suffix_from_url() {
    let out = super::migrate_config_from_env(&[(
        "VEX_MODEL_URL",
        "https://api.example.internal/v1/messages",
    )]);
    assert!(
        out.contains("model_url = \"https://api.example.internal\""),
        "{out}"
    );
    assert!(!out.contains("/v1/messages"), "{out}");
}

#[test]
fn test_migrate_empty_env_produces_only_header_comments() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _api_protocol = EnvRestore::capture("VEX_API_PROTOCOL");
    let _structured_tool_protocol = EnvRestore::capture("VEX_STRUCTURED_TOOL_PROTOCOL");
    let _model_url = EnvRestore::capture("VEX_MODEL_URL");
    std::env::remove_var("VEX_API_PROTOCOL");
    std::env::remove_var("VEX_STRUCTURED_TOOL_PROTOCOL");
    std::env::remove_var("VEX_MODEL_URL");

    let out = super::migrate_config_from_env(&[]);
    assert!(
        out.starts_with("# generated by vex migrate config"),
        "{out}"
    );
    assert_eq!(out.lines().count(), 2);
}
