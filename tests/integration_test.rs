use reqwest::header::HeaderMap;
use vexcoder::batch_mode::{build_batch_runtime, AutoApproveScope, BatchRunOpts, OutputFormat};
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
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        model_headers: HeaderMap::new(),
        notes_path: None,
        hooks: Vec::new(),
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
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        model_headers: HeaderMap::new(),
        notes_path: None,
        hooks: Vec::new(),
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

#[test]
fn test_config_rejects_notes_path_in_repo_local_config() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let cwd = repo_root.join("sub");
    std::fs::create_dir_all(repo_root.join(".vex")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        repo_root.join(".vex/config.toml"),
        "notes_path = \"/tmp/notes.md\"\n",
    )
    .unwrap();
    let err = Config::load_for_tests(&cwd, None, None).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("notes_path"),
        "expected 'notes_path' in error: {msg}"
    );
}

#[test]
fn test_hook_repo_local_config_rejected_at_load() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let cwd = repo_root.join("sub");
    std::fs::create_dir_all(repo_root.join(".vex")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        repo_root.join(".vex/config.toml"),
        "[[hooks]]\nevent = \"post_tool\"\ntool = \"write_file\"\ncommand = \"echo\"\nargs = []\non_fail = \"warn\"\n",
    )
    .unwrap();
    let err = Config::load_for_tests(&cwd, None, None).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("[[hooks]]") || msg.contains("hooks"),
        "expected hooks diagnostic in error: {msg}"
    );
}

// -- PE-01 / PE-02 public API contract -----------------------------------------
//
// The six async anchor tests that depend on MockApiClient live in
// src/batch_mode.rs #[cfg(test)] (MockApiClient is not pub to integration tests).
// These tests cover the integration-layer contract using only pub API.

#[test]
fn test_batch_run_opts_default_format_is_jsonl() {
    let opts = BatchRunOpts::default();
    assert!(
        matches!(opts.format, OutputFormat::Jsonl),
        "BatchRunOpts::default() must produce OutputFormat::Jsonl per ADR-024 PE-01"
    );
    assert!(
        opts.max_turns.is_none(),
        "default must impose no turn limit"
    );
    assert!(opts.auto_approve.is_none(), "default must not auto-approve");
}

#[test]
fn test_batch_auto_approve_scope_once_and_task_are_distinct() {
    assert_ne!(
        format!("{:?}", AutoApproveScope::Once),
        format!("{:?}", AutoApproveScope::Task),
    );
}

#[test]
fn test_batch_output_format_jsonl_and_text_are_distinct() {
    assert_ne!(
        format!("{:?}", OutputFormat::Jsonl),
        format!("{:?}", OutputFormat::Text),
    );
}

#[tokio::test]
async fn test_build_batch_runtime_succeeds_with_local_config() {
    let _lock = crate::test_support::ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let config = vexcoder::config::Config {
        model_token: None,
        model_name: "local/test-model".to_string(),
        model_url: "http://localhost:11434/v1/messages".to_string(),
        working_dir: temp.path().to_path_buf(),
        model_backend: vexcoder::runtime::ModelBackendKind::LocalRuntime,
        model_protocol: vexcoder::runtime::ModelProtocol::MessagesV1,
        tool_call_mode: vexcoder::runtime::ToolCallMode::TaggedFallback,
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        model_headers: HeaderMap::new(),
        notes_path: None,
        hooks: Vec::new(),
    };
    let result = build_batch_runtime(&config, "test task".to_string(), BatchRunOpts::default());
    assert!(
        result.is_ok(),
        "build_batch_runtime must succeed without a live server: {:?}",
        result.err()
    );
}
