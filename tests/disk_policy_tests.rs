use std::path::Path;

use vexcoder::disk_policy::{
    check_path, enforce, enforce_runtime, resolve_policy_mode, DiskPermission, DiskPolicyMode,
};

mod test_support {
    pub static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
}

#[test]
fn classifies_allowed_disk_paths() {
    assert_eq!(
        check_path(Path::new(".vex/index/chunks.bin")),
        DiskPermission::SearchIndex
    );
    assert_eq!(
        check_path(Path::new("/repo/.vex/state/task-1.json")),
        DiskPermission::TaskStateMap
    );
}

#[test]
fn classifies_workspace_source_as_forbidden() {
    assert_eq!(
        check_path(Path::new("src/lib.rs")),
        DiskPermission::Forbidden
    );
}

#[test]
fn strict_mode_rejects_forbidden_paths() {
    let error = enforce(Path::new("src/lib.rs"), DiskPolicyMode::Strict)
        .expect_err("strict mode must reject workspace source access");
    assert!(error.to_string().contains("forbidden disk access"));
}

#[test]
fn strict_mode_allows_search_index_paths() {
    let permission = enforce(Path::new(".vex/index/chunks.bin"), DiskPolicyMode::Strict)
        .expect("strict mode must allow search index paths");
    assert_eq!(permission, DiskPermission::SearchIndex);
}

#[test]
fn warn_mode_keeps_running_for_forbidden_paths() {
    let permission = enforce(Path::new("src/lib.rs"), DiskPolicyMode::Warn)
        .expect("warn mode should not hard-fail");
    assert_eq!(permission, DiskPermission::Forbidden);
}

#[test]
fn runtime_mode_defaults_to_off_when_env_missing() {
    let _lock = test_support::ENV_LOCK.blocking_lock();
    std::env::remove_var("VEX_DISK_POLICY");

    assert_eq!(resolve_policy_mode(), DiskPolicyMode::Off);
    let permission = enforce_runtime(Path::new("src/lib.rs"))
        .expect("off mode should not hard-fail on forbidden paths");
    assert_eq!(permission, DiskPermission::Forbidden);
}

#[test]
fn runtime_mode_uses_strict_env() {
    let _lock = test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_DISK_POLICY", "strict");

    let error = enforce_runtime(Path::new("src/lib.rs"))
        .expect_err("strict env mode must reject forbidden paths");
    assert!(error.to_string().contains("forbidden disk access"));

    std::env::remove_var("VEX_DISK_POLICY");
}

#[test]
fn runtime_mode_uses_warn_env() {
    let _lock = test_support::ENV_LOCK.blocking_lock();
    std::env::set_var("VEX_DISK_POLICY", "warn");

    let permission = enforce_runtime(Path::new("src/lib.rs"))
        .expect("warn env mode should not hard-fail on forbidden paths");
    assert_eq!(permission, DiskPermission::Forbidden);

    std::env::remove_var("VEX_DISK_POLICY");
}

#[test]
fn windows_backslash_index_path_is_search_index() {
    let p = std::path::PathBuf::from(".vex\\index\\chunks.bin");
    assert_eq!(check_path(&p), DiskPermission::SearchIndex);
}

#[test]
fn windows_backslash_state_path_is_task_state_map() {
    let p = std::path::PathBuf::from(".vex\\state\\task-001.json");
    assert_eq!(check_path(&p), DiskPermission::TaskStateMap);
}

#[test]
fn windows_mixed_separator_path_is_search_index() {
    let p = std::path::PathBuf::from(".vex\\index/data.bin");
    assert_eq!(check_path(&p), DiskPermission::SearchIndex);
}

#[test]
fn index_prefix_without_path_separator_is_forbidden() {
    // Regression: paths like ".vex/indexing.txt" must not match SearchIndex.
    assert_eq!(
        check_path(Path::new(".vex/indexing.txt")),
        DiskPermission::Forbidden,
    );
    assert_eq!(
        check_path(Path::new("/repo/.vex/indexed-data")),
        DiskPermission::Forbidden,
    );
}

#[test]
fn state_prefix_without_path_separator_is_forbidden() {
    // Regression: paths like ".vex/stateful.bin" must not match TaskStateMap.
    assert_eq!(
        check_path(Path::new(".vex/stateful.bin")),
        DiskPermission::Forbidden,
    );
}
