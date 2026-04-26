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
            unsafe { std::env::set_var(key, val) }
        }
        #[allow(unsafe_code)]
        pub fn remove_var(&self, key: &str) {
            unsafe { std::env::remove_var(key) }
        }
    }
    pub static ENV_LOCK: EnvLock = EnvLock::new();
}

fn make_local_config(temp: &std::path::Path) -> Config {
    Config {
        model_token: None,
        model_name: "local/test-model".to_string(),
        model_url: "http://localhost:8080/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: temp.to_path_buf(),
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
    }
}

#[test]
fn config_validation_rejects_remote_endpoint_and_allows_local() {
    let temp = tempfile::tempdir().unwrap();
    let mut remote = make_local_config(temp.path());
    remote.model_token = Some("test-key".to_string());
    remote.model_url = "https://model.example.internal/v1/messages".to_string();
    remote.model_backend = ModelBackendKind::ApiServer;
    assert!(
        remote.validate().is_err(),
        "remote model without https must be rejected"
    );

    assert!(
        make_local_config(temp.path()).validate().is_ok(),
        "local endpoint must be allowed"
    );
}

#[test]
fn config_layering_env_wins_over_repo_and_user_wins_over_system() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    _lock.remove_var("VEX_MODEL_NAME");
    _lock.remove_var("VEX_MODEL_URL");
    let temp = tempfile::tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let cwd = repo_root.join("sub");
    std::fs::create_dir_all(repo_root.join(".vex")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        repo_root.join(".vex/config.toml"),
        "model_name = \"repo-model\"\nmodel_url = \"http://repo.example/v1\"\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("user.toml"),
        "model_name = \"user-model\"\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("system.toml"),
        "model_name = \"system-model\"\n",
    )
    .unwrap();

    _lock.set_var("VEX_MODEL_NAME", "env-model");
    let cfg = Config::load_for_tests(
        &cwd,
        Some(&temp.path().join("user.toml")),
        Some(&temp.path().join("system.toml")),
    )
    .unwrap();
    assert_eq!(cfg.model_name, "env-model", "env must win");
    assert_eq!(
        cfg.model_url, "http://repo.example/v1",
        "repo must supply url"
    );
    _lock.remove_var("VEX_MODEL_NAME");

    let cfg2 = Config::load_for_tests(
        &cwd,
        Some(&temp.path().join("user.toml")),
        Some(&temp.path().join("system.toml")),
    )
    .unwrap();
    assert_eq!(cfg2.model_name, "repo-model", "repo must win over user");
    _lock.remove_var("VEX_MODEL_NAME");
    _lock.remove_var("VEX_MODEL_URL");
}

#[test]
fn config_rejects_security_sensitive_and_unknown_keys_in_any_layer() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("project");
    std::fs::create_dir_all(&cwd).unwrap();

    let cases = [
        (
            "model_token.toml",
            "model_token = \"secret\"\n",
            "model_token",
        ),
        (
            "unknown.toml",
            "model_name = \"ok\"\nunknown_key = \"bad\"\n",
            "user.toml",
        ),
    ];
    for (filename, content, expected_in_err) in &cases {
        let user_cfg = temp.path().join(filename);
        std::fs::write(&user_cfg, content).unwrap();
        let err = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap_err();
        assert!(
            format!("{err:#}").contains(expected_in_err),
            "{filename}: unexpected error: {err:#}"
        );
    }

    let repo_root = temp.path().join("repo");
    let repo_cwd = repo_root.join("sub");
    std::fs::create_dir_all(repo_root.join(".vex")).unwrap();
    std::fs::create_dir_all(&repo_cwd).unwrap();
    for (content, key) in [
        ("notes_path = \"/tmp/notes.md\"\n", "notes_path"),
        (
            "[[hooks]]\nevent = \"post_tool\"\ntool = \"write_file\"\ncommand = \"echo\"\nargs = []\non_fail = \"warn\"\n",
            "hooks",
        ),
    ] {
        std::fs::write(repo_root.join(".vex/config.toml"), content).unwrap();
        let err = Config::load_for_tests(&repo_cwd, None, None).unwrap_err();
        assert!(
            format!("{err:#}").contains(key),
            "{key}: unexpected error: {err:#}"
        );
    }
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
    std::path::PathBuf::from("bash")
}

fn run_forbidden_names_check(repo_root: &std::path::Path) -> std::process::Output {
    let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts/check_forbidden_names.sh");
    std::process::Command::new(forbidden_names_shell())
        .arg(script)
        .current_dir(repo_root)
        .output()
        .unwrap()
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
fn allowlisted_setup_workflow_content() -> String {
    "name: Hosted Agent Setup Checks\njobs:\n  hosted-agent-setup-checks:\n    runs-on: ubuntu-latest\n".to_string()
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
fn forbidden_names_check_blocks_violations_and_allows_allowlisted_files() {
    let temp = prepare_forbidden_names_fixture();
    std::fs::write(
        temp.path().join("src/prompts/coder_system.txt"),
        forbidden_prompt_content(),
    )
    .unwrap();
    let out = run_forbidden_names_check(temp.path());
    assert!(
        !out.status.success(),
        "must fail on forbidden prompt content"
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("src/prompts/coder_system.txt"),
        "must report the offending file: {text}"
    );

    let temp2 = prepare_forbidden_names_fixture();
    let fname = forbidden_model_filename();
    std::fs::write(
        temp2.path().join("models").join(&fname),
        "name = \"api-structured\"\n",
    )
    .unwrap();
    let out2 = run_forbidden_names_check(temp2.path());
    assert!(
        !out2.status.success(),
        "must fail on forbidden model filename"
    );

    let temp3 = prepare_forbidden_names_fixture();
    std::fs::write(
        temp3
            .path()
            .join(".github/workflows/hosted-agent-setup-checks.yml"),
        allowlisted_setup_workflow_content(),
    )
    .unwrap();
    std::fs::write(
        temp3
            .path()
            .join(".github/agents/vexcoder-ui-parity-orchestrator.agent.md"),
        allowlisted_agent_profile_content(),
    )
    .unwrap();
    let out3 = run_forbidden_names_check(temp3.path());
    let text3 = format!(
        "{}{}",
        String::from_utf8_lossy(&out3.stdout),
        String::from_utf8_lossy(&out3.stderr)
    );
    assert!(
        out3.status.success(),
        "allowlisted files must pass: {text3}"
    );
}

#[tokio::test]
async fn batch_runtime_builds_successfully_with_correct_default_opts() {
    let _lock = crate::test_support::ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let opts = BatchRunOpts::default();
    assert!(matches!(opts.format, OutputFormat::Jsonl));
    assert!(opts.max_turns.is_none() && opts.auto_approve.is_none());
    assert_ne!(
        format!("{:?}", AutoApproveScope::Once),
        format!("{:?}", AutoApproveScope::Task)
    );

    let result = build_batch_runtime(
        &make_local_config(temp.path()),
        "test task".to_string(),
        BatchRunOpts::default(),
    );
    assert!(
        result.is_ok(),
        "build_batch_runtime must succeed without a running server: {:?}",
        result.err()
    );
}
