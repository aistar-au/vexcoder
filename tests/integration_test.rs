use reqwest::header::HeaderMap;
use vexcoder::batch_mode::{AutoApproveScope, BatchRunOpts, OutputFormat, build_batch_runtime};
use vexcoder::config::Config;
use vexcoder::runtime::{ModelBackendKind, ModelProtocol, ToolCallMode, ToolPolicy};
use vexcoder::types::ModelProfile;

mod test_support {
    pub struct EnvLock(tokio::sync::Mutex<()>);
    impl EnvLock {
        pub const fn new() -> Self {
            Self(tokio::sync::Mutex::const_new(()))
        }
        pub fn blocking_lock(&self) -> EnvLockGuard<'_> {
            EnvLockGuard {
                _guard: self.0.blocking_lock(),
            }
        }
        pub async fn lock(&self) -> EnvLockGuard<'_> {
            EnvLockGuard {
                _guard: self.0.lock().await,
            }
        }
    }
    pub struct EnvLockGuard<'a> {
        _guard: tokio::sync::MutexGuard<'a, ()>,
    }
    impl EnvLockGuard<'_> {
        #[allow(unsafe_code)]
        pub fn set_var(&self, key: &str, val: impl AsRef<std::ffi::OsStr>) {
            // SAFETY: the guard proves exclusive ownership of ENV_LOCK.
            unsafe { std::env::set_var(key, val) }
        }
        #[allow(unsafe_code)]
        pub fn remove_var(&self, key: &str) {
            // SAFETY: the guard proves exclusive ownership of ENV_LOCK.
            unsafe { std::env::remove_var(key) }
        }
    }
    pub static ENV_LOCK: EnvLock = EnvLock::new();
}

fn prepare_forbidden_names_fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    for dir in [
        "TASKS",
        "adr",
        "docs/src",
        ".github/agents",
        "src/prompts",
        "tests",
        "scripts",
        ".github/workflows",
        "models",
    ] {
        std::fs::create_dir_all(temp.path().join(dir)).unwrap();
    }
    for file in [".gitignore", "AGENTS.md", "CONTRIBUTING.md", "Makefile"] {
        std::fs::write(temp.path().join(file), "\n").unwrap();
    }
    temp
}

fn forbidden_names_shell() -> std::path::PathBuf {
    if cfg!(windows) {
        for var in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(root) = std::env::var_os(var) {
                let root = std::path::PathBuf::from(root);
                for candidate in [
                    root.join("Git").join("bin").join("bash.exe"),
                    root.join("Git").join("usr").join("bin").join("bash.exe"),
                ] {
                    if candidate.is_file() {
                        return candidate;
                    }
                }
            }
        }
    }

    std::path::PathBuf::from("bash")
}

fn windows_ripgrep_binary() -> Option<std::path::PathBuf> {
    if !cfg!(windows) {
        return None;
    }

    if let Ok(output) = std::process::Command::new("where").arg("rg.exe").output()
        && output.status.success()
        && let Some(path) = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
    {
        return Some(std::path::PathBuf::from(path));
    }

    [
        std::env::var_os("CARGO_HOME")
            .map(std::path::PathBuf::from)
            .map(|root| root.join("bin").join("rg.exe")),
        std::env::var_os("USERPROFILE")
            .map(std::path::PathBuf::from)
            .map(|root| root.join(".cargo").join("bin").join("rg.exe")),
    ]
    .into_iter()
    .flatten()
    .find(|path| path.is_file())
}

fn run_forbidden_names_check(repo_root: &std::path::Path) -> std::process::Output {
    let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts/check_forbidden_names.sh");
    let mut command = std::process::Command::new(forbidden_names_shell());
    command.arg(script).current_dir(repo_root);

    if let Some(rg_binary) = windows_ripgrep_binary() {
        command.env("VEX_RG_BIN", rg_binary.to_string_lossy().replace('\\', "/"));
    }

    command.output().unwrap()
}

fn forbidden_prompt_content() -> String {
    String::from_utf8(vec![
        0x67, 0x6f, 0x6f, 0x67, 0x6c, 0x65, 0x2d, 0x62, 0x72, 0x61, 0x6e, 0x64, 0x65, 0x64, 0x20,
        0x70, 0x72, 0x6f, 0x6d, 0x70, 0x74, 0x20, 0x74, 0x65, 0x78, 0x74, 0x0a,
    ])
    .unwrap()
}

fn forbidden_model_filename() -> String {
    String::from_utf8(vec![
        0x71, 0x77, 0x65, 0x6e, 0x2d, 0x63, 0x6f, 0x64, 0x65, 0x72, 0x2e, 0x74, 0x6f, 0x6d, 0x6c,
    ])
    .unwrap()
}

fn allowlisted_setup_workflow_filename() -> String {
    "hosted-agent-setup-checks.yml".to_string()
}

fn allowlisted_setup_workflow_content() -> String {
    "name: Hosted Agent Setup Checks\njobs:\n  hosted-agent-setup-checks:\n    runs-on: ubuntu-latest\n"
        .to_string()
}

fn allowlisted_focus_agent_profile_content() -> String {
    String::from_utf8(vec![
        0x2d, 0x2d, 0x2d, 0x0a, 0x6d, 0x6f, 0x64, 0x65, 0x6c, 0x3a, 0x20, 0x22, 0x47, 0x50, 0x54,
        0x2d, 0x35, 0x2e, 0x34, 0x22, 0x0a, 0x2d, 0x2d, 0x2d, 0x0a, 0x55, 0x73, 0x65, 0x20, 0x74,
        0x68, 0x65, 0x20, 0x66, 0x6f, 0x63, 0x75, 0x73, 0x65, 0x64, 0x20, 0x72, 0x65, 0x70, 0x6f,
        0x73, 0x69, 0x74, 0x6f, 0x72, 0x79, 0x2d, 0x6c, 0x65, 0x76, 0x65, 0x6c, 0x20, 0x63, 0x6f,
        0x64, 0x69, 0x6e, 0x67, 0x20, 0x61, 0x67, 0x65, 0x6e, 0x74, 0x20, 0x70, 0x72, 0x6f, 0x66,
        0x69, 0x6c, 0x65, 0x2e, 0x0a,
    ])
    .unwrap()
}

fn allowlisted_agent_profile_content() -> String {
    String::from_utf8(vec![
        0x2d, 0x2d, 0x2d, 0x0a, 0x6d, 0x6f, 0x64, 0x65, 0x6c, 0x3a, 0x20, 0x22, 0x43, 0x6c, 0x61,
        0x75, 0x64, 0x65, 0x20, 0x4f, 0x70, 0x75, 0x73, 0x20, 0x34, 0x2e, 0x36, 0x22, 0x0a, 0x2d,
        0x2d, 0x2d, 0x0a, 0x55, 0x73, 0x65, 0x20, 0x74, 0x68, 0x65, 0x20, 0x72, 0x65, 0x70, 0x6f,
        0x73, 0x69, 0x74, 0x6f, 0x72, 0x79, 0x2d, 0x6c, 0x65, 0x76, 0x65, 0x6c, 0x20, 0x63, 0x6f,
        0x64, 0x69, 0x6e, 0x67, 0x20, 0x61, 0x67, 0x65, 0x6e, 0x74, 0x20, 0x70, 0x72, 0x6f, 0x66,
        0x69, 0x6c, 0x65, 0x2e, 0x0a,
    ])
    .unwrap()
}

#[test]
fn test_config_validation_rejects_local_model_for_remote_endpoint() {
    let config = Config {
        model_token: Some("test-key".to_string()),
        model_name: "local/mock-model".to_string(),
        model_url: "https://model.example.internal/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: std::env::current_dir().expect("cwd"),
        model_backend: ModelBackendKind::ApiServer,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::Structured,
        tool_policy: ToolPolicy::Full,
        model_profile: ModelProfile::default_for_backend(ModelBackendKind::ApiServer),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: vexcoder::runtime::SandboxConfig::default(),
        model_headers: HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: vexcoder::config::CompactionConfig::default(),
        undo: vexcoder::config::UndoConfig::default(),
        search: vexcoder::config::SearchConfig::default(),
        notes_path: None,
        api: vexcoder::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: vexcoder::config::AutoMemoryConfig::default(),
        api_client: vexcoder::config::ApiClientConfig::default(),
        force: false,
        bypass_policy: false,
        expand_context: false,
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_allows_local_endpoint_without_token() {
    let config = Config {
        model_token: None,
        model_name: "local/default-3.3".to_string(),
        model_url: "http://localhost:8000/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: std::env::current_dir().expect("cwd"),
        model_backend: ModelBackendKind::LocalRuntime,
        model_protocol: ModelProtocol::MessagesV1,
        tool_call_mode: ToolCallMode::Structured,
        tool_policy: ToolPolicy::Full,
        model_profile: ModelProfile::default_for_backend(ModelBackendKind::LocalRuntime),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: vexcoder::runtime::SandboxConfig::default(),
        model_headers: HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: vexcoder::config::CompactionConfig::default(),
        undo: vexcoder::config::UndoConfig::default(),
        search: vexcoder::config::SearchConfig::default(),
        notes_path: None,
        api: vexcoder::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: vexcoder::config::AutoMemoryConfig::default(),
        api_client: vexcoder::config::ApiClientConfig::default(),
        force: false,
        bypass_policy: false,
        expand_context: false,
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
    _lock.set_var("VEX_MODEL_NAME", "env-model");
    let cfg = Config::load_for_tests(&cwd, Some(&user_cfg), Some(&system_cfg)).unwrap();
    assert_eq!(cfg.model_name, "env-model");
    assert_eq!(cfg.model_url, "http://repo.example/v1");
    _lock.remove_var("VEX_MODEL_NAME");
}

#[test]
fn test_config_repo_overrides_user_system_and_defaults() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    _lock.remove_var("VEX_MODEL_NAME");
    _lock.remove_var("VEX_MODEL_URL");
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
    _lock.remove_var("VEX_MODEL_NAME");
    _lock.remove_var("VEX_MODEL_URL");
}

#[test]
fn test_config_user_overrides_system_and_defaults() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    _lock.remove_var("VEX_MODEL_NAME");
    _lock.remove_var("VEX_MODEL_URL");
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("project");
    let user_cfg = temp.path().join("user.toml");
    let system_cfg = temp.path().join("system.toml");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(&user_cfg, "model_name = \"user-model\"\n").unwrap();
    std::fs::write(&system_cfg, "model_name = \"system-model\"\n").unwrap();

    let cfg = Config::load_for_tests(&cwd, Some(&user_cfg), Some(&system_cfg)).unwrap();
    assert_eq!(cfg.model_name, "user-model");
    _lock.remove_var("VEX_MODEL_NAME");
    _lock.remove_var("VEX_MODEL_URL");
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
fn check_forbidden_names_sh_blocks_proprietary_name_in_prompts_dir() {
    let temp = prepare_forbidden_names_fixture();
    std::fs::write(
        temp.path().join("src/prompts/coder_system.txt"),
        forbidden_prompt_content(),
    )
    .unwrap();

    let output = run_forbidden_names_check(temp.path());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        !output.status.success(),
        "expected forbidden-name check to fail for prompt content: {text}"
    );
    assert!(
        text.contains("src/prompts/coder_system.txt"),
        "expected prompt path in output: {text}"
    );
}

#[test]
fn check_forbidden_names_sh_blocks_proprietary_name_in_model_filename() {
    let temp = prepare_forbidden_names_fixture();
    let forbidden_filename = forbidden_model_filename();
    std::fs::write(
        temp.path().join("models").join(&forbidden_filename),
        "name = \"api-structured\"\n",
    )
    .unwrap();

    let output = run_forbidden_names_check(temp.path());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        !output.status.success(),
        "expected forbidden-name check to fail for model filename: {text}"
    );
    assert!(
        text.contains(&format!("models/{forbidden_filename}")),
        "expected model path in output: {text}"
    );
}

#[test]
fn check_forbidden_names_sh_allows_required_coding_agent_setup_workflow() {
    let temp = prepare_forbidden_names_fixture();
    std::fs::write(
        temp.path()
            .join(".github/workflows")
            .join(allowlisted_setup_workflow_filename()),
        allowlisted_setup_workflow_content(),
    )
    .unwrap();

    let output = run_forbidden_names_check(temp.path());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        output.status.success(),
        "expected allowlisted setup workflow to pass forbidden-name check: {text}"
    );
}

#[test]
fn check_forbidden_names_sh_allows_repository_ui_orchestrator_agent_profile() {
    let temp = prepare_forbidden_names_fixture();
    std::fs::write(
        temp.path()
            .join(".github/agents/vexcoder-ui-parity-orchestrator.agent.md"),
        allowlisted_agent_profile_content(),
    )
    .unwrap();

    let output = run_forbidden_names_check(temp.path());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        output.status.success(),
        "expected allowlisted repository agent profile to pass forbidden-name check: {text}"
    );
}

#[test]
fn check_forbidden_names_sh_allows_repository_ui_paragraph_agent_profile() {
    let temp = prepare_forbidden_names_fixture();
    std::fs::write(
        temp.path()
            .join(".github/agents/vexcoder-ui-paragraph-renderer.agent.md"),
        allowlisted_focus_agent_profile_content(),
    )
    .unwrap();

    let output = run_forbidden_names_check(temp.path());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        output.status.success(),
        "expected allowlisted paragraph agent profile to pass forbidden-name check: {text}"
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

#[test]
fn test_batch_run_opts_default_format_is_jsonl() {
    let opts = BatchRunOpts::default();
    assert!(
        matches!(opts.format, OutputFormat::Jsonl),
        "BatchRunOpts::default() must produce OutputFormat::Jsonl per ADR-024 PE-01"
    );
    assert!(
        opts.max_turns.is_none(),
        "default must impose no pulse limit"
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
        model_url: "http://localhost:8080/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: temp.path().to_path_buf(),
        model_backend: vexcoder::runtime::ModelBackendKind::LocalRuntime,
        model_protocol: vexcoder::runtime::ModelProtocol::MessagesV1,
        tool_call_mode: vexcoder::runtime::ToolCallMode::Structured,
        tool_policy: vexcoder::runtime::ToolPolicy::Full,
        model_profile: ModelProfile::default_for_backend(
            vexcoder::runtime::ModelBackendKind::LocalRuntime,
        ),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: vexcoder::runtime::SandboxConfig::default(),
        model_headers: HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: vexcoder::config::CompactionConfig::default(),
        undo: vexcoder::config::UndoConfig::default(),
        search: vexcoder::config::SearchConfig::default(),
        notes_path: None,
        api: vexcoder::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: vexcoder::config::AutoMemoryConfig::default(),
        api_client: vexcoder::config::ApiClientConfig::default(),
        force: false,
        bypass_policy: false,
        expand_context: false,
    };
    let result = build_batch_runtime(&config, "test task".to_string(), BatchRunOpts::default());
    assert!(
        result.is_ok(),
        "build_batch_runtime must succeed without a running server: {:?}",
        result.err()
    );
}
