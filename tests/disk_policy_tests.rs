use std::path::Path;
use vexcoder::disk_policy::{
    DiskPermission, DiskPolicyMode, check_path, enforce, enforce_runtime, resolve_policy_mode,
};

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

#[test]
fn check_path_classifies_allowed_and_forbidden_patterns() {
    assert_eq!(
        check_path(Path::new(".vex/index/chunks.bin")),
        DiskPermission::SearchIndex
    );
    assert_eq!(
        check_path(Path::new("/repo/.vex/state/task-1.json")),
        DiskPermission::TaskStateMap
    );
    assert_eq!(
        check_path(Path::new("src/lib.rs")),
        DiskPermission::Forbidden
    );
    assert_eq!(
        check_path(Path::new(".vex/indexing.txt")),
        DiskPermission::Forbidden,
        "index prefix without separator must be forbidden"
    );
    assert_eq!(
        check_path(Path::new(".vex/stateful.bin")),
        DiskPermission::Forbidden,
        "state prefix without separator must be forbidden"
    );
    assert_eq!(
        check_path(&std::path::PathBuf::from(".vex\\index\\chunks.bin")),
        DiskPermission::SearchIndex,
        "windows backslash index"
    );
    assert_eq!(
        check_path(&std::path::PathBuf::from(".vex\\state\\task-001.json")),
        DiskPermission::TaskStateMap,
        "windows backslash state"
    );
}

#[test]
fn enforce_respects_strict_and_warn_modes() {
    let err = enforce(Path::new("src/lib.rs"), DiskPolicyMode::Strict).unwrap_err();
    assert!(err.to_string().contains("forbidden disk access"));

    let perm = enforce(Path::new(".vex/index/chunks.bin"), DiskPolicyMode::Strict).unwrap();
    assert_eq!(perm, DiskPermission::SearchIndex);

    let perm2 = enforce(Path::new("src/lib.rs"), DiskPolicyMode::Warn).unwrap();
    assert_eq!(perm2, DiskPermission::Forbidden);
}

#[test]
fn enforce_runtime_reads_env_var_and_defaults_to_off() {
    let _lock = test_support::ENV_LOCK.blocking_lock();
    _lock.remove_var("VEX_DISK_POLICY");
    assert_eq!(resolve_policy_mode(), DiskPolicyMode::Off);
    assert_eq!(
        enforce_runtime(Path::new("src/lib.rs")).unwrap(),
        DiskPermission::Forbidden,
        "off mode must not hard-fail"
    );

    _lock.set_var("VEX_DISK_POLICY", "strict");
    assert!(
        enforce_runtime(Path::new("src/lib.rs")).is_err(),
        "strict env must reject forbidden paths"
    );
    _lock.remove_var("VEX_DISK_POLICY");

    _lock.set_var("VEX_DISK_POLICY", "warn");
    assert_eq!(
        enforce_runtime(Path::new("src/lib.rs")).unwrap(),
        DiskPermission::Forbidden,
        "warn env must not hard-fail"
    );
    _lock.remove_var("VEX_DISK_POLICY");
}
