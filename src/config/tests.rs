use super::{Config, ModelBackendKind};
use std::path::PathBuf;

use crate::test_support::EnvRestore;
use crate::types::ModelProfile;
use rstest::rstest;

// URL validation: three cases share identical setup — lock, set three env vars, load, validate.
#[rstest]
#[case::rejects_non_loopback("http://api.example.internal/v1/messages", "remote-model", Some("token"), false, "https://")]
#[case::allows_loopback("http://127.0.0.1:8080/v1/messages", "local-model", None, true, "")]
#[case::allows_private_network("http://192.168.1.100:11434/v1", "local-model", None, true, "")]
fn model_url_validation(
    #[case] url: &str,
    #[case] name: &str,
    #[case] token: Option<&str>,
    #[case] expect_ok: bool,
    #[case] err_contains: &str,
) {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _url_r = EnvRestore::capture(&_lock, "VEX_MODEL_URL");
    let _name_r = EnvRestore::capture(&_lock, "VEX_MODEL_NAME");
    let _token_r = EnvRestore::capture(&_lock, "VEX_MODEL_TOKEN");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_URL", url);
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_NAME", name);
    match token {
        Some(t) => crate::test_support::test_set_var(&_lock, "VEX_MODEL_TOKEN", t),
        None => crate::test_support::test_remove_var(&_lock, "VEX_MODEL_TOKEN"),
    }
    let cfg = Config::load().expect("load failed");
    if expect_ok {
        assert!(cfg.validate().is_ok());
    } else {
        let error = cfg.validate().expect_err("must be rejected");
        assert!(error.to_string().contains(err_contains), "{error:#}");
    }
}

#[test]
fn test_config_validate_accepts_api_client_base_url_without_model_url() {
    let mut config = Config::default_for_tui();
    config.model_url.clear();
    config.model_name = "local/test-model".to_string();
    config.model_token = None;
    config.api_client.base_url = "http://127.0.0.1:8787".to_string();

    assert!(
        config.validate().is_ok(),
        "api_client.base_url should satisfy validation when model_url is unset"
    );
}

#[test]
fn test_api_client_config_defaults_probe_timeout_ms() {
    let config = crate::config::ApiClientConfig::default();
    assert_eq!(config.probe_timeout_ms, 2000);
}

// explicit_protocol field: one acceptance case and two rejection cases share identical TOML
// setup; only the field value and expected outcome differ.
#[rstest]
#[case::accepts_chat_compat("chat-compat", None)]
#[case::rejects_short_alias("chat", Some("invalid api_client.explicit_protocol"))]
#[case::rejects_obsolete_alias("choices_delta", Some("api_client.explicit_protocol"))]
fn explicit_protocol_field_validation(#[case] value: &str, #[case] err_contains: Option<&str>) {
    let cwd = tempfile::tempdir().unwrap();
    let user_cfg_dir = tempfile::tempdir().unwrap();
    let user_cfg_file = user_cfg_dir.path().join("config.toml");
    std::fs::write(
        &user_cfg_file,
        format!(
            "model_url = \"http://127.0.0.1:8000/v1\"\n\
             model_name = \"local-model\"\n\
             [api_client]\n\
             base_url = \"http://127.0.0.1:8787\"\n\
             explicit_protocol = \"{value}\"\n"
        ),
    )
    .unwrap();
    match err_contains {
        None => {
            let config = Config::load_for_tests(cwd.path(), Some(&user_cfg_file), None).unwrap();
            assert_eq!(
                config.api_client.explicit_protocol,
                Some(crate::runtime::ModelProtocol::ChatCompat)
            );
        }
        Some(substr) => {
            let error =
                Config::load_for_tests(cwd.path(), Some(&user_cfg_file), None).unwrap_err();
            assert!(format!("{error:#}").contains(substr), "unexpected error: {error:#}");
        }
    }
}

#[test]
fn test_config_loads_vex_model_name_without_previous_prefix() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_URL", "http://localhost:8080/v1");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_NAME", "local-model-70b");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_TOKEN");

    let cfg = Config::load().expect("load failed");
    assert!(
        cfg.validate().is_ok(),
        "neutral model name must pass validation"
    );
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_URL");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_NAME");
}

#[test]
fn test_model_backend_kind_parses_from_env_var() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_BACKEND", "local-runtime");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_URL", "http://localhost:8080/v1");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_NAME", "local-model");
    let cfg = Config::load().expect("load failed");
    assert!(cfg.validate().is_ok());
    assert_eq!(cfg.model_backend, ModelBackendKind::LocalRuntime);
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_BACKEND");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_URL");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_NAME");
}

#[test]
fn test_invalid_model_protocol_env_var_is_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_URL", "http://localhost:8080/v1");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_NAME", "mock-model");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_PROTOCOL", "unsupported-value");

    assert!(Config::load().is_err());

    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_URL");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_NAME");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_PROTOCOL");
}

// Security-sensitive keys must be rejected in repo-scoped .vex/config.toml to prevent
// accidental secret leakage through version control.
#[rstest]
#[case("[api]\nkey = \"literal-secret\"\n", "api.key")]
#[case("model_url_skip_tls_check = true\n", "model_url_skip_tls_check")]
#[case("[api]\ntls_skip_verify = true\n", "tls_skip_verify")]
#[case("[api]\nvpn_trust = true\n", "vpn_trust")]
fn repo_local_security_sensitive_keys_are_rejected(#[case] config_toml: &str, #[case] key: &str) {
    let temp = tempfile::tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let cwd = repo_root.join("nested/project");
    std::fs::create_dir_all(repo_root.join(".vex")).unwrap();
    std::fs::create_dir_all(repo_root.join(".git")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(repo_root.join(".vex/config.toml"), config_toml).unwrap();
    let error = Config::load_for_tests(&cwd, None, None).unwrap_err();
    assert!(format!("{error:#}").contains(key), "unexpected error: {error:#}");
}

#[test]
fn test_user_api_key_env_reference_resolves() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _api_key = EnvRestore::capture(&_lock, "VEX_API_KEY");
    crate::test_support::test_set_var(&_lock, "VEX_API_KEY", "resolved-secret");

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
fn test_invalid_model_backend_error_lists_supported_values() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_BACKEND", "unsupported-value");

    let err = super::read_env_layer().unwrap_err();
    let msg = format!("{err:#}");

    assert!(
        msg.contains("local-runtime, api-server"),
        "expected supported values in error: {msg}"
    );
    assert!(
        !msg.contains("local_runtime"),
        "unexpected alias in error: {msg}"
    );
    assert!(
        !msg.contains("api_server"),
        "unexpected alias in error: {msg}"
    );
    assert!(!msg.contains("remote"), "unexpected alias in error: {msg}");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_BACKEND");
}

#[test]
fn test_obsolete_model_backend_alias_is_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_BACKEND", "remote");

    let err = super::read_env_layer().unwrap_err();

    assert!(
        format!("{err:#}").contains("Invalid VEX_MODEL_BACKEND"),
        "unexpected error: {err:#}"
    );
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_BACKEND");
}

#[test]
fn test_invalid_model_protocol_error_lists_supported_values() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_PROTOCOL", "unsupported-value");

    let err = super::read_env_layer().unwrap_err();
    let msg = format!("{err:#}");

    assert!(
        msg.contains("messages-v1, chat-compat"),
        "expected supported values in error: {msg}"
    );
    assert!(
        !msg.contains("messages_v1"),
        "unexpected alias in error: {msg}"
    );
    assert!(
        !msg.contains("chat_compat"),
        "unexpected alias in error: {msg}"
    );
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_PROTOCOL");
}

#[test]
fn test_obsolete_model_protocol_alias_is_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_PROTOCOL", "chat");

    let err = super::read_env_layer().unwrap_err();

    assert!(
        format!("{err:#}").contains("Invalid VEX_MODEL_PROTOCOL"),
        "unexpected error: {err:#}"
    );
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_PROTOCOL");
}

#[test]
fn test_obsolete_tool_call_mode_alias_is_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::test_support::test_set_var(&_lock, "VEX_TOOL_CALL_MODE", "structured_tool_calls");

    let err = super::read_env_layer().unwrap_err();

    assert!(
        format!("{err:#}").contains("Invalid VEX_TOOL_CALL_MODE"),
        "unexpected error: {err:#}"
    );
    crate::test_support::test_remove_var(&_lock, "VEX_TOOL_CALL_MODE");
}

#[test]
fn test_invalid_tool_call_mode_error_lists_supported_value() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::test_support::test_set_var(&_lock, "VEX_TOOL_CALL_MODE", "unsupported-value");

    let err = super::read_env_layer().unwrap_err();
    let msg = format!("{err:#}");

    assert!(
        msg.contains("structured"),
        "expected supported value in error: {msg}"
    );
    assert!(
        !msg.contains("structured_tool_calls"),
        "unexpected obsolete alias in error: {msg}"
    );
    crate::test_support::test_remove_var(&_lock, "VEX_TOOL_CALL_MODE");
}

#[test]
fn test_user_config_path_prefers_xdg_config_home() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _home = EnvRestore::capture(&_lock, "HOME");
    let _xdg = EnvRestore::capture(&_lock, "XDG_CONFIG_HOME");
    let temp = tempfile::tempdir().unwrap();
    let xdg_root = temp.path().join("xdg-root");
    let fallback_home = temp.path().join("home");
    let xdg_path = xdg_root.join("vex").join("config.toml");
    let fallback_path = fallback_home.join(".vex").join("config.toml");

    std::fs::create_dir_all(xdg_path.parent().unwrap()).unwrap();
    std::fs::create_dir_all(fallback_path.parent().unwrap()).unwrap();
    std::fs::write(&xdg_path, "model_name = \"xdg\"\n").unwrap();
    std::fs::write(&fallback_path, "model_name = \"fallback\"\n").unwrap();
    crate::test_support::test_set_var(&_lock, "HOME", &fallback_home);
    crate::test_support::test_set_var(&_lock, "XDG_CONFIG_HOME", &xdg_root);

    assert_eq!(super::user_config_path(), Some(xdg_path));
}

#[test]
fn test_user_config_path_ignores_old_home_config() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _home = EnvRestore::capture(&_lock, "HOME");
    let _xdg = EnvRestore::capture(&_lock, "XDG_CONFIG_HOME");
    let temp = tempfile::tempdir().unwrap();
    let xdg_root = temp.path().join("xdg-root");
    let home = temp.path().join("home");
    let xdg_path = xdg_root.join("vex").join("config.toml");
    let old_home_path = home.join(".vex").join("config.toml");

    std::fs::create_dir_all(old_home_path.parent().unwrap()).unwrap();
    std::fs::write(&old_home_path, "model_name = \"fallback\"\n").unwrap();
    crate::test_support::test_set_var(&_lock, "HOME", &home);
    crate::test_support::test_set_var(&_lock, "XDG_CONFIG_HOME", &xdg_root);

    assert_eq!(super::user_config_path(), Some(xdg_path));
}

#[test]
fn test_doctor_rollup_matches_runtime_working_dir_resolution() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _workdir = EnvRestore::capture(&_lock, "VEX_WORKDIR");
    let temp = tempfile::tempdir().unwrap();
    crate::test_support::test_set_var(&_lock, "VEX_WORKDIR", "~/doctor-workdir");

    let config = Config::load_for_tests(temp.path(), None, None).unwrap();
    let snapshot = super::doctor_rollup(temp.path()).unwrap();

    assert_eq!(config.working_dir, PathBuf::from("~/doctor-workdir"));
    assert_eq!(snapshot.working_dir, config.working_dir);
    crate::test_support::test_remove_var(&_lock, "VEX_WORKDIR");
}

#[test]
fn test_doctor_rollup_respects_env_sandbox_require_override() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _sandbox_require = EnvRestore::capture(&_lock, "VEX_SANDBOX_REQUIRE");
    let temp = tempfile::tempdir().unwrap();
    let user_cfg = temp.path().join("user.toml");
    let cwd = temp.path().join("repo");

    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(&user_cfg, "sandbox_require = false\n").unwrap();
    crate::test_support::test_set_var(&_lock, "VEX_SANDBOX_REQUIRE", "true");

    let snapshot = super::doctor_rollup(&cwd).unwrap();
    assert!(snapshot.sandbox_require);

    crate::test_support::test_remove_var(&_lock, "VEX_SANDBOX_REQUIRE");
}

#[rstest]
#[case::valid(Some(r#"{"x-custom-header": "value1", "x-other": "value2"}"#), true, Some(2))]
#[case::invalid_header_name(Some(r#"{"invalid header!": "v"}"#), false, None)]
#[case::non_string_value(Some(r#"{"x-count": 42}"#), false, None)]
#[case::empty_env(None, true, Some(0))]
fn parse_model_headers_json_validation(
    #[case] env_value: Option<&str>,
    #[case] expect_ok: bool,
    #[case] expected_len: Option<usize>,
) {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _restore = EnvRestore::capture(&_lock, "VEX_MODEL_HEADERS_JSON");
    match env_value {
        Some(v) => crate::test_support::test_set_var(&_lock, "VEX_MODEL_HEADERS_JSON", v),
        None => crate::test_support::test_remove_var(&_lock, "VEX_MODEL_HEADERS_JSON"),
    }
    let result = super::parse_model_headers_json();
    assert_eq!(result.is_ok(), expect_ok);
    if let (Ok(headers), Some(len)) = (result, expected_len) {
        assert_eq!(headers.len(), len);
    }
}

// Token budget fields: env set, absent (default), and zero (uses default) share setup.
#[rstest]
#[case(Some("2048"), 2048usize)]
#[case(None, 4096)]
#[case(Some("0"), 4096)]
fn max_project_instructions_tokens_from_env(
    #[case] env_value: Option<&str>,
    #[case] expected: usize,
) {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _url_r = EnvRestore::capture(&_lock, "VEX_MODEL_URL");
    let _name_r = EnvRestore::capture(&_lock, "VEX_MODEL_NAME");
    let _key_r = EnvRestore::capture(&_lock, "VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_URL", "http://localhost:8080/v1");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_NAME", "test-model");
    match env_value {
        Some(v) => {
            crate::test_support::test_set_var(&_lock, "VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS", v)
        }
        None => crate::test_support::test_remove_var(&_lock, "VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS"),
    }
    let cfg = Config::load().expect("load failed");
    assert_eq!(cfg.max_project_instructions_tokens, expected);
}

#[rstest]
#[case(Some("1024"), 1024usize)]
#[case(None, 2048)]
#[case(Some("0"), 2048)]
fn max_memory_tokens_from_env(#[case] env_value: Option<&str>, #[case] expected: usize) {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _url_r = EnvRestore::capture(&_lock, "VEX_MODEL_URL");
    let _name_r = EnvRestore::capture(&_lock, "VEX_MODEL_NAME");
    let _key_r = EnvRestore::capture(&_lock, "VEX_MAX_MEMORY_TOKENS");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_URL", "http://localhost:8080/v1");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_NAME", "test-model");
    match env_value {
        Some(v) => crate::test_support::test_set_var(&_lock, "VEX_MAX_MEMORY_TOKENS", v),
        None => crate::test_support::test_remove_var(&_lock, "VEX_MAX_MEMORY_TOKENS"),
    }
    let cfg = Config::load().expect("load failed");
    assert_eq!(cfg.max_memory_tokens, expected);
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
    let _skip = EnvRestore::capture(&_lock, "VEX_MODEL_URL_SKIP_TLS_CHECK");
    let _url = EnvRestore::capture(&_lock, "VEX_MODEL_URL");
    let _name = EnvRestore::capture(&_lock, "VEX_MODEL_NAME");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_URL_SKIP_TLS_CHECK", "true");
    crate::test_support::test_set_var(
        &_lock,
        "VEX_MODEL_URL",
        "https://localhost:8443/v1/messages",
    );
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_NAME", "test-model");

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
fn test_unknown_toml_key_is_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    let user_cfg = temp.path().join("user.toml");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(&user_cfg, "imaginary_key = true\n").unwrap();

    let error = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap_err();
    let msg = format!("{error:#}");
    assert!(
        msg.contains("unknown") || msg.contains("Unknown"),
        "expected unknown-key error, got: {msg}"
    );
}

#[test]
fn test_model_token_in_config_file_is_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    let user_cfg = temp.path().join("user.toml");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(&user_cfg, "model_token = \"secret\"\n").unwrap();

    let error = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap_err();
    let msg = format!("{error:#}");
    assert!(
        msg.contains("model_token"),
        "expected model_token error, got: {msg}"
    );
}

#[test]
fn test_malformed_toml_is_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    let user_cfg = temp.path().join("user.toml");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(&user_cfg, "{{{{ not valid toml").unwrap();

    let error = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap_err();
    let msg = format!("{error:#}");
    assert!(
        msg.contains("TOML") || msg.contains("toml"),
        "expected TOML error, got: {msg}"
    );
}

#[test]
fn test_missing_config_file_is_not_an_error() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    let nonexistent = temp.path().join("nonexistent.toml");

    let result = Config::load_for_tests(&cwd, Some(&nonexistent), None);
    assert!(
        result.is_ok(),
        "missing config file should be silently ignored"
    );
}

// Invalid enum-valued config keys: each case writes a single bad value and checks that
// the error message identifies both the field name and the rejected value.
#[rstest]
#[case("model_backend = \"bogus\"\n", "model_backend", "bogus")]
#[case("model_protocol = \"bogus\"\n", "model_protocol", "bogus")]
#[case("sandbox = \"bogus\"\n", "sandbox", "bogus")]
#[case("sandbox = \"bwrap\"\n", "sandbox", "bwrap")]
#[case("[api]\ntransport = \"bogus\"\n", "transport", "bogus")]
fn invalid_config_file_enum_values_are_rejected(
    #[case] config_toml: &str,
    #[case] err_key: &str,
    #[case] err_val: &str,
) {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    let user_cfg = temp.path().join("user.toml");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(&user_cfg, config_toml).unwrap();
    let error = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap_err();
    let msg = format!("{error:#}");
    assert!(msg.contains(err_key) && msg.contains(err_val), "unexpected error: {msg}");
}

#[test]
fn test_user_layer_overrides_system_layer() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _url = EnvRestore::capture(&_lock, "VEX_MODEL_URL");
    let _name = EnvRestore::capture(&_lock, "VEX_MODEL_NAME");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_URL");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_NAME");

    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    let system_cfg = temp.path().join("system.toml");
    let user_cfg = temp.path().join("user.toml");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(&system_cfg, "model_name = \"system-model\"\n").unwrap();
    std::fs::write(&user_cfg, "model_name = \"user-model\"\n").unwrap();

    let cfg = Config::load_for_tests(&cwd, Some(&user_cfg), Some(&system_cfg)).unwrap();
    assert_eq!(cfg.model_name, "user-model");
}

#[test]
fn test_repo_layer_overrides_user_layer() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _url = EnvRestore::capture(&_lock, "VEX_MODEL_URL");
    let _name = EnvRestore::capture(&_lock, "VEX_MODEL_NAME");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_URL");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_NAME");

    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    let user_cfg = temp.path().join("user.toml");
    std::fs::create_dir_all(cwd.join(".vex")).unwrap();
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(
        cwd.join(".vex/config.toml"),
        "model_name = \"repo-model\"\n",
    )
    .unwrap();
    std::fs::write(&user_cfg, "model_name = \"user-model\"\n").unwrap();

    let cfg = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap();
    assert_eq!(cfg.model_name, "repo-model");
}

#[test]
fn test_env_layer_overrides_repo_layer() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _url = EnvRestore::capture(&_lock, "VEX_MODEL_URL");
    let _name = EnvRestore::capture(&_lock, "VEX_MODEL_NAME");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_NAME", "env-model");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_URL");

    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    std::fs::create_dir_all(cwd.join(".vex")).unwrap();
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(
        cwd.join(".vex/config.toml"),
        "model_name = \"repo-model\"\n",
    )
    .unwrap();

    let cfg = Config::load_for_tests(&cwd, None, None).unwrap();
    assert_eq!(cfg.model_name, "env-model");
}

#[test]
fn test_repo_local_notes_path_is_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    std::fs::create_dir_all(cwd.join(".vex")).unwrap();
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(
        cwd.join(".vex/config.toml"),
        "notes_path = \"~/notes.md\"\n",
    )
    .unwrap();

    let error = Config::load_for_tests(&cwd, None, None).unwrap_err();
    let msg = format!("{error:#}");
    assert!(
        msg.contains("notes_path"),
        "expected notes_path rejection, got: {msg}"
    );
}

#[test]
fn test_repo_local_hooks_are_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    std::fs::create_dir_all(cwd.join(".vex")).unwrap();
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(
        cwd.join(".vex/config.toml"),
        "[[hooks]]\nevent = \"pre_tool\"\ncommand = \"echo hi\"\n",
    )
    .unwrap();

    let error = Config::load_for_tests(&cwd, None, None).unwrap_err();
    let msg = format!("{error:#}");
    assert!(
        msg.contains("hooks"),
        "expected hooks rejection, got: {msg}"
    );
}

#[test]
fn test_repo_local_mcp_servers_are_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    std::fs::create_dir_all(cwd.join(".vex")).unwrap();
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(
        cwd.join(".vex/config.toml"),
        "[[mcp_servers]]\nname = \"test\"\ntransport = \"stdio\"\ncommand = \"echo\"\n",
    )
    .unwrap();

    let error = Config::load_for_tests(&cwd, None, None).unwrap_err();
    let msg = format!("{error:#}");
    assert!(
        msg.contains("mcp_servers") || msg.contains("MCP"),
        "expected mcp_servers rejection, got: {msg}"
    );
}

#[test]
fn test_system_mcp_servers_are_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    let system_cfg = temp.path().join("system.toml");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(
        &system_cfg,
        "[[mcp_servers]]\nname = \"test\"\ntransport = \"stdio\"\ncommand = \"echo\"\n",
    )
    .unwrap();

    let error = Config::load_for_tests(&cwd, None, Some(&system_cfg)).unwrap_err();
    let msg = format!("{error:#}");
    assert!(
        msg.contains("mcp_servers") || msg.contains("MCP"),
        "expected system mcp_servers rejection, got: {msg}"
    );
}

// MCP server constraint validation: name rules and transport/field consistency checks.
// Each case writes a single TOML snippet and checks that the error message identifies the issue.
#[rstest]
#[case(
    "[[mcp_servers]]\nname = \"dup\"\ntransport = \"stdio\"\ncommand = \"echo\"\n\n\
     [[mcp_servers]]\nname = \"dup\"\ntransport = \"stdio\"\ncommand = \"echo2\"\n",
    "duplicate"
)]
#[case("[[mcp_servers]]\nname = \"\"\ntransport = \"stdio\"\ncommand = \"echo\"\n", "empty")]
#[case(
    "[[mcp_servers]]\nname = \"bad name!\"\ntransport = \"stdio\"\ncommand = \"echo\"\n",
    "invalid characters"
)]
#[case("[[mcp_servers]]\nname = \"nocommand\"\ntransport = \"stdio\"\n", "command")]
#[case("[[mcp_servers]]\nname = \"nourl\"\ntransport = \"http\"\n", "url")]
#[case(
    "[[mcp_servers]]\nname = \"badstdio\"\ntransport = \"stdio\"\ncommand = \"echo\"\nurl = \"http://example.com\"\n",
    "url"
)]
#[case(
    "[[mcp_servers]]\nname = \"badhttp\"\ntransport = \"http\"\nurl = \"http://example.com\"\ncommand = \"echo\"\n",
    "command"
)]
#[case(
    "[[mcp_servers]]\nname = \"badhttp\"\ntransport = \"http\"\nurl = \"http://example.com\"\nargs = [\"--option\"]\n",
    "args"
)]
fn mcp_server_constraint_violations_are_rejected(
    #[case] config_toml: &str,
    #[case] expected_err: &str,
) {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    let user_cfg = temp.path().join("user.toml");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(&user_cfg, config_toml).unwrap();
    let error = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap_err();
    let msg = format!("{error:#}");
    assert!(msg.contains(expected_err), "unexpected error for {config_toml:?}: {msg}");
}

#[test]
fn test_valid_mcp_stdio_server_loads() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    let user_cfg = temp.path().join("user.toml");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(
        &user_cfg,
        "[[mcp_servers]]\nname = \"my-server\"\ntransport = \"stdio\"\ncommand = \"mcp-tool\"\nargs = [\"--port\", \"8080\"]\n",
    )
    .unwrap();

    let cfg = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap();
    assert_eq!(cfg.mcp_servers.len(), 1);
    assert_eq!(cfg.mcp_servers[0].name, "my-server");
    assert_eq!(cfg.mcp_servers[0].command.as_deref(), Some("mcp-tool"));
    assert_eq!(cfg.mcp_servers[0].args, vec!["--port", "8080"]);
}

#[test]
fn test_valid_mcp_http_server_loads() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    let user_cfg = temp.path().join("user.toml");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(
        &user_cfg,
        "[[mcp_servers]]\nname = \"remote\"\ntransport = \"http\"\nurl = \"http://localhost:3000/mcp\"\n",
    )
    .unwrap();

    let cfg = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap();
    assert_eq!(cfg.mcp_servers.len(), 1);
    assert_eq!(cfg.mcp_servers[0].name, "remote");
    assert_eq!(
        cfg.mcp_servers[0].url.as_deref(),
        Some("http://localhost:3000/mcp")
    );
    assert!(cfg.mcp_servers[0].command.is_none());
}

#[rstest]
#[case::completions_url("https://api.example.internal/v1/chat/completions")]
#[case::v1_url("https://api.example.internal/v1")]
#[case::messages_url("https://api.example.internal/v1/messages")]
#[case::empty_url("")]
#[case::transposed_messages_v1("http://127.0.0.1:8000/messages/v1")]
fn infer_model_protocol_always_returns_messages_v1(#[case] url: &str) {
    assert_eq!(
        super::infer_model_protocol(url),
        crate::runtime::ModelProtocol::MessagesV1
    );
}

#[rstest]
#[case::empty_url("", ModelBackendKind::LocalRuntime)]
#[case::localhost("http://localhost:8080/v1", ModelBackendKind::LocalRuntime)]
#[case::remote("https://api.example.internal/v1", ModelBackendKind::ApiServer)]
fn default_model_backend_from_url(#[case] url: &str, #[case] expected: ModelBackendKind) {
    assert_eq!(super::default_model_backend(url), expected);
}

#[test]
fn test_default_tool_call_mode_structured_for_local() {
    assert_eq!(
        super::default_tool_call_mode("http://127.0.0.1:8080/v1"),
        crate::runtime::ToolCallMode::Structured
    );
}

#[test]
fn test_default_tool_call_mode_structured_for_remote() {
    assert_eq!(
        super::default_tool_call_mode("https://api.example.internal/v1"),
        crate::runtime::ToolCallMode::Structured
    );
}

#[test]
fn test_empty_config_resolves_compiled_defaults() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _url = EnvRestore::capture(&_lock, "VEX_MODEL_URL");
    let _name = EnvRestore::capture(&_lock, "VEX_MODEL_NAME");
    let _backend = EnvRestore::capture(&_lock, "VEX_MODEL_BACKEND");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_URL");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_NAME");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_BACKEND");

    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();

    let cfg = Config::load_for_tests(&cwd, None, None).unwrap();
    assert_eq!(cfg.model_name, "local/default");
    assert_eq!(cfg.model_backend, ModelBackendKind::LocalRuntime);
    assert_eq!(cfg.max_project_instructions_tokens, 4096);
    assert_eq!(cfg.max_memory_tokens, 2048);
    assert!(cfg.hooks.is_empty());
    assert!(cfg.mcp_servers.is_empty());
}

#[test]
fn test_working_dir_defaults_to_cwd() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _workdir = EnvRestore::capture(&_lock, "VEX_WORKDIR");
    crate::test_support::test_remove_var(&_lock, "VEX_WORKDIR");

    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();

    let cfg = Config::load_for_tests(&cwd, None, None).unwrap();
    assert_eq!(cfg.working_dir, cwd);
}

#[test]
fn test_find_repo_local_config_walks_ancestors() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _url = EnvRestore::capture(&_lock, "VEX_MODEL_URL");
    let _name = EnvRestore::capture(&_lock, "VEX_MODEL_NAME");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_URL");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_NAME");

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let nested = root.join("deep/nested/dir");
    std::fs::create_dir_all(root.join(".vex")).unwrap();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        root.join(".vex/config.toml"),
        "model_name = \"ancestor-found\"\n",
    )
    .unwrap();

    let cfg = Config::load_for_tests(&nested, None, None).unwrap();
    assert_eq!(cfg.model_name, "ancestor-found");
}

#[test]
fn test_empty_env_model_token_is_treated_as_absent() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _token = EnvRestore::capture(&_lock, "VEX_MODEL_TOKEN");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_TOKEN", "   ");

    let (_, token) = super::read_env_layer().unwrap();
    assert!(token.is_none(), "whitespace-only token should be None");
}

#[test]
fn test_model_token_falls_back_to_keyring_reader_when_env_missing() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _token = EnvRestore::capture(&_lock, "VEX_MODEL_TOKEN");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_TOKEN");

    let token = super::model_token_from_env_or_keyring_with(|_| Ok(Some("keyring-token".into())));
    assert_eq!(token.as_deref(), Some("keyring-token"));
}

#[test]
fn test_model_token_prefers_non_empty_env_over_keyring_reader() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _token = EnvRestore::capture(&_lock, "VEX_MODEL_TOKEN");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_TOKEN", "env-token");

    let token = super::model_token_from_env_or_keyring_with(|_| Ok(Some("keyring-token".into())));
    assert_eq!(token.as_deref(), Some("env-token"));
}

#[test]
fn test_invalid_bool_flag_is_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _skip = EnvRestore::capture(&_lock, "VEX_MODEL_URL_SKIP_TLS_CHECK");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_URL_SKIP_TLS_CHECK", "maybe");

    let error = super::read_env_layer().unwrap_err();
    let msg = format!("{error:#}");
    assert!(
        msg.contains("VEX_MODEL_URL_SKIP_TLS_CHECK"),
        "expected env var name in error, got: {msg}"
    );
}

#[test]
fn test_invalid_api_port_is_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _port = EnvRestore::capture(&_lock, "VEX_API_PORT");
    crate::test_support::test_set_var(&_lock, "VEX_API_PORT", "not-a-number");

    let error = super::read_env_layer().unwrap_err();
    let msg = format!("{error:#}");
    assert!(
        msg.contains("VEX_API_PORT"),
        "expected VEX_API_PORT in error, got: {msg}"
    );
}

#[test]
fn test_repo_local_http_hooks_are_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    std::fs::create_dir_all(cwd.join(".vex")).unwrap();
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(
        cwd.join(".vex/config.toml"),
        "[[http_hooks]]\nevent = \"post_tool\"\ntool = \"apply_patch\"\nurl = \"https://example.com/hook\"\n",
    )
    .unwrap();

    let error = Config::load_for_tests(&cwd, None, None).unwrap_err();
    let msg = format!("{error:#}");
    assert!(
        msg.contains("http_hooks"),
        "expected http_hooks rejection, got: {msg}"
    );
}

#[test]
fn test_http_hook_invalid_url_rejected() {
    use super::hooks::HookEvent;
    use super::hooks::HttpHookConfig;

    let hook = HttpHookConfig {
        event: HookEvent::PostTool,
        tool: "apply_patch".to_string(),
        url: "ftp://nothttp.example.com".to_string(),
        on_fail: super::hooks::HookOnFail::Warn,
    };
    let error = hook.validate().unwrap_err();
    let msg = format!("{error:#}");
    assert!(
        msg.contains("https://") || msg.contains("http://"),
        "expected URL scheme error, got: {msg}"
    );
}

#[test]
fn test_http_hook_valid_url_accepted() {
    use super::hooks::HookEvent;
    use super::hooks::HttpHookConfig;

    let hook = HttpHookConfig {
        event: HookEvent::PostTool,
        tool: "apply_patch".to_string(),
        url: "https://ci.example.com/webhook".to_string(),
        on_fail: super::hooks::HookOnFail::Warn,
    };
    assert!(hook.validate().is_ok());
}

#[test]
fn test_http_hook_loaded_from_user_config() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    let user_cfg = temp.path().join("user.toml");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(
        &user_cfg,
        concat!(
            "model_url = \"http://localhost:8080/v1\"\n",
            "[[http_hooks]]\n",
            "event = \"post_tool\"\n",
            "tool = \"apply_patch\"\n",
            "url = \"https://ci.example.com/webhook\"\n",
            "on_fail = \"warn\"\n",
        ),
    )
    .unwrap();

    let config = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap();
    assert_eq!(config.http_hooks.len(), 1);
    assert_eq!(config.http_hooks[0].url, "https://ci.example.com/webhook");
    assert_eq!(config.http_hooks[0].tool, "apply_patch");
}

#[test]
fn test_interactive_model_selection_preserves_protocol_for_transposed_url() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_BACKEND");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_NAME");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_PROTOCOL");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_TOKEN");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_URL");

    let cwd = tempfile::tempdir().unwrap();
    let user_cfg = tempfile::tempdir().unwrap();
    let user_cfg_file = user_cfg.path().join("config.toml");
    std::fs::write(
        &user_cfg_file,
        "model_url = \"http://127.0.0.1:8000/v1/messages\"\n",
    )
    .unwrap();

    let mut config = Config::load_for_tests(cwd.path(), Some(&user_cfg_file), None).unwrap();
    config.apply_interactive_model_selection(
        "http://127.0.0.1:8000/messages/v1".to_string(),
        Some("test-model".to_string()),
    );

    assert_eq!(
        config.model_protocol,
        crate::runtime::ModelProtocol::MessagesV1
    );
}
