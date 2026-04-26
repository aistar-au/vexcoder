use super::Config;
use crate::test_support::EnvRestore;
use rstest::rstest;

#[rstest]
#[case::rejects_non_loopback(
    "http://api.example.internal/v1/messages",
    "remote-model",
    Some("token"),
    false,
    "https://"
)]
#[case::allows_loopback("http://127.0.0.1:8080/v1/messages", "local-model", None, true, "")]
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
        assert!(
            cfg.validate()
                .unwrap_err()
                .to_string()
                .contains(err_contains)
        );
    }
}

#[rstest]
#[case("[api]\nkey = \"literal-secret\"\n", "api.key")]
#[case("[[hooks]]\nevent = \"pre_tool\"\ncommand = \"echo hi\"\n", "hooks")]
#[case(
    "[[mcp_servers]]\nname = \"s\"\ntransport = \"stdio\"\ncommand = \"tool\"\n",
    "mcp_servers"
)]
fn repo_local_security_sensitive_keys_are_rejected(#[case] config_toml: &str, #[case] key: &str) {
    let temp = tempfile::tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let cwd = repo_root.join("sub");
    std::fs::create_dir_all(repo_root.join(".vex")).unwrap();
    std::fs::create_dir_all(repo_root.join(".git")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(repo_root.join(".vex/config.toml"), config_toml).unwrap();
    let err = Config::load_for_tests(&cwd, None, None).unwrap_err();
    assert!(
        format!("{err:#}").contains(key),
        "unexpected error: {err:#}"
    );
}

#[rstest]
#[case("model_backend = \"bogus\"\n", "model_backend", "bogus")]
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
    let err = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains(err_key) && msg.contains(err_val),
        "unexpected: {msg}"
    );
}

#[rstest]
#[case(
    "[[mcp_servers]]\nname = \"dup\"\ntransport = \"stdio\"\ncommand = \"echo\"\n\n[[mcp_servers]]\nname = \"dup\"\ntransport = \"stdio\"\ncommand = \"echo2\"\n",
    "duplicate"
)]
#[case(
    "[[mcp_servers]]\nname = \"nocommand\"\ntransport = \"stdio\"\n",
    "command"
)]
#[case("[[mcp_servers]]\nname = \"nourl\"\ntransport = \"http\"\n", "url")]
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
    let err = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap_err();
    assert!(
        format!("{err:#}").contains(expected_err),
        "unexpected: {err:#}"
    );
}

#[test]
fn token_limit_env_vars_accept_valid_and_default_on_zero() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _url_r = EnvRestore::capture(&_lock, "VEX_MODEL_URL");
    let _name_r = EnvRestore::capture(&_lock, "VEX_MODEL_NAME");
    let _pi = EnvRestore::capture(&_lock, "VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS");
    let _mm = EnvRestore::capture(&_lock, "VEX_MAX_MEMORY_TOKENS");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_URL", "http://localhost:8080/v1");
    crate::test_support::test_set_var(&_lock, "VEX_MODEL_NAME", "test-model");
    crate::test_support::test_set_var(&_lock, "VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS", "2048");
    crate::test_support::test_set_var(&_lock, "VEX_MAX_MEMORY_TOKENS", "1024");
    let cfg = Config::load().expect("load failed");
    assert_eq!(cfg.max_project_instructions_tokens, 2048);
    assert_eq!(cfg.max_memory_tokens, 1024);
    crate::test_support::test_set_var(&_lock, "VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS", "0");
    crate::test_support::test_set_var(&_lock, "VEX_MAX_MEMORY_TOKENS", "0");
    let cfg2 = Config::load().expect("load failed");
    assert_eq!(cfg2.max_project_instructions_tokens, 4096);
    assert_eq!(cfg2.max_memory_tokens, 2048);
}

#[test]
fn layered_config_user_overrides_system_and_walks_ancestor_directories() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let _url = EnvRestore::capture(&_lock, "VEX_MODEL_URL");
    let _name = EnvRestore::capture(&_lock, "VEX_MODEL_NAME");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_URL");
    crate::test_support::test_remove_var(&_lock, "VEX_MODEL_NAME");
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::write(
        temp.path().join("system.toml"),
        "model_name = \"system-model\"\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("user.toml"),
        "model_name = \"user-model\"\n",
    )
    .unwrap();
    let cfg = Config::load_for_tests(
        &cwd,
        Some(&temp.path().join("user.toml")),
        Some(&temp.path().join("system.toml")),
    )
    .unwrap();
    assert_eq!(cfg.model_name, "user-model");

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
    assert_eq!(
        Config::load_for_tests(&nested, None, None)
            .unwrap()
            .model_name,
        "ancestor-found"
    );
}

#[test]
fn config_file_parse_errors_are_rejected() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("repo");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();

    let user_cfg = temp.path().join("model_token.toml");
    std::fs::write(&user_cfg, "model_token = \"secret\"\n").unwrap();
    let err = Config::load_for_tests(&cwd, Some(&user_cfg), None).unwrap_err();
    assert!(format!("{err:#}").contains("model_token"));

    let bad_cfg = temp.path().join("bad.toml");
    std::fs::write(&bad_cfg, "{{{{ not valid toml").unwrap();
    let err = Config::load_for_tests(&cwd, Some(&bad_cfg), None).unwrap_err();
    assert!(format!("{err:#}").to_lowercase().contains("toml"));

    let unk_cfg = temp.path().join("unk.toml");
    std::fs::write(&unk_cfg, "imaginary_key = true\n").unwrap();
    let err = Config::load_for_tests(&cwd, Some(&unk_cfg), None).unwrap_err();
    assert!(format!("{err:#}").to_lowercase().contains("unknown"));
}
