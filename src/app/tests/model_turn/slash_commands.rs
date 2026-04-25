use super::*;

#[tokio::test]
async fn model_command_echoes_current_name_and_switches_on_valid_arg() {
    let mut ctx = setup_ctx();
    let mut mode = TuiMode::new();
    mode.on_user_input("/model".to_string(), &mut ctx);
    assert!(mode.history_lines().iter().any(|l| l.contains("[model]")));

    let old = mode.model_name.clone();
    mode.on_user_input("/model local/coder-8b".to_string(), &mut ctx);
    assert_eq!(mode.model_name, "local/coder-8b");
    assert!(mode.history_lines().iter().any(|l| l.contains(&old) && l.contains("local/coder-8b")));
    assert!(!mode.is_pulse_in_progress(), "/model must not start a pulse");
}

#[tokio::test]
async fn model_command_rejects_backend_mismatch() {
    let mut ctx = setup_ctx();
    let mut config = Config::default_for_tui();
    config.model_backend = crate::runtime::ModelBackendKind::ApiServer;
    let mut mode = TuiMode::new_with_config(None, config);
    mode.on_user_input("/model local/phi-3".to_string(), &mut ctx);
    assert_ne!(mode.model_name, "local/phi-3");
    assert!(mode.history_lines().iter().any(|l| l.contains("rejected")));
}

#[tokio::test]
async fn diff_command_renders_git_working_tree_diff() {
    let mut ctx = setup_ctx();
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    std::fs::write(temp.path().join("src.rs"), "fn a() {}").unwrap();
    git_success(temp.path(), &["add", "src.rs"]);
    git_success(temp.path(), &["commit", "-m", "init"]);
    std::fs::write(temp.path().join("src.rs"), "fn a() {} fn b() {}").unwrap();

    let config = config_with_workdir(temp.path());
    let mut mode = TuiMode::new_with_config(None, config);
    mode.on_user_input("/diff".to_string(), &mut ctx);
    let lines = mode.history_lines();
    assert!(lines.iter().any(|l| l.contains("src.rs") || l.contains("diff") || l.contains("b()")),
        "diff output must mention changed file; lines: {lines:?}");
}
