use reqwest::header::HeaderMap;
use vexcoder::config::Config;
use vexcoder::runtime::{ModelBackendKind, ModelProtocol, ToolCallMode};

// Each integration test binary needs its own ENV_LOCK to serialise
// env-var mutations across tests within this binary.
mod test_support {
    pub static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
}

#[test]
fn test_config_validation_rejects_local_model_for_remote_endpoint() {
    let config = Config {
        model_token: Some("test-key".to_string()),
        model_name: "local/mock-model".to_string(),
        model_url: "https://model.example.internal/v1/messages".to_string(),
        working_dir: std::env::current_dir().expect("cwd"),
        model_backend: ModelBackendKind::ApiServer,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::Structured,
        model_headers: HeaderMap::new(),
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_allows_local_endpoint_without_token() {
    let config = Config {
        model_token: None,
        model_name: "local/llama3.3".to_string(),
        model_url: "http://localhost:8000/v1/messages".to_string(),
        working_dir: std::env::current_dir().expect("cwd"),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::TaggedFallback,
        model_headers: HeaderMap::new(),
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_config_prefers_env_over_repo_user_system_and_defaults() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let cwd = repo_root.join("nested/project");
    let user_cfg = temp.path().join("user-config.toml");
    let system_cfg = temp.path().join("system-config.toml");
    std::fs::create_dir_all(repo_root.join(".vex")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        repo_root.join(".vex/config.toml"),
        "model_name = \"repo-model\"\nmodel_url = \"http://repo.example/v1\"\n",
    )
    .unwrap();
    std::fs::write(&user_cfg, "model_name = \"user-model\"\n").unwrap();
    std::fs::write(&system_cfg, "model_name = \"system-model\"\n").unwrap();
    std::env::set_var("VEX_MODEL_NAME", "env-model");
    let cfg = Config::load_for_tests(&cwd, Some(&user_cfg), Some(&system_cfg)).unwrap();
    assert_eq!(cfg.model_name, "env-model");
    assert_eq!(cfg.model_url, "http://repo.example/v1");
    std::env::remove_var("VEX_MODEL_NAME");
}

#[test]
fn test_config_repo_overrides_user_system_and_defaults() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::remove_var("VEX_MODEL_NAME");
    std::env::remove_var("VEX_MODEL_URL");
    let temp = tempfile::tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let cwd = repo_root.join("sub");
    let user_cfg = temp.path().join("user.toml");
    let system_cfg = temp.path().join("system.toml");
    std::fs::create_dir_all(repo_root.join(".vex")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        repo_root.join(".vex/config.toml"),
        "model_name = \"repo-model\"\n",
    )
    .unwrap();
    std::fs::write(&user_cfg, "model_name = \"user-model\"\n").unwrap();
    std::fs::write(&system_cfg, "model_name = \"system-model\"\n").unwrap();
    let cfg = Config::load_for_tests(&cwd, Some(&user_cfg), Some(&system_cfg)).unwrap();
    assert_eq!(cfg.model_name, "repo-model");
    std::env::remove_var("VEX_MODEL_NAME");
    std::env::remove_var("VEX_MODEL_URL");
}

#[test]
fn test_config_user_overrides_system_and_defaults() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    std::env::remove_var("VEX_MODEL_NAME");
    std::env::remove_var("VEX_MODEL_URL");
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("project");
    let user_cfg = temp.path().join("user.toml");
    let system_cfg = temp.path().join("system.toml");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(&user_cfg, "model_name = \"user-model\"\n").unwrap();
    std::fs::write(&system_cfg, "model_name = \"system-model\"\n").unwrap();
    // No repo-local config in cwd ancestry.
    let cfg = Config::load_for_tests(&cwd, Some(&user_cfg), Some(&system_cfg)).unwrap();
    assert_eq!(cfg.model_name, "user-model");
    std::env::remove_var("VEX_MODEL_NAME");
    std::env::remove_var("VEX_MODEL_URL");
}

#[test]
fn test_config_rejects_model_token_in_toml() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let user_cfg = temp.path().join("user.toml");
    let cwd = temp.path().join("project");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(&user_cfg, "model_token = \"secret\"\n").unwrap();
    let err = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("model_token"),
        "expected 'model_token' in error: {msg}"
    );
    assert!(
        msg.contains("user.toml"),
        "expected file name in error: {msg}"
    );
}

#[test]
fn test_config_rejects_unknown_toml_keys() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let user_cfg = temp.path().join("user.toml");
    let cwd = temp.path().join("project");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(&user_cfg, "model_name = \"ok\"\nunknown_key = \"bad\"\n").unwrap();
    let err = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("user.toml"),
        "expected file name in error: {msg}"
    );
}
