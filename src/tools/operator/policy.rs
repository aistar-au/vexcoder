use std::path::Path;

use crate::disk_policy::{self, DiskPermission};

// This module currently enforces only the durable-state access constraints
// introduced by ADR-038. ADR-048 reserves the same seam for the later
// permissions-overlay evaluator so protected-path checks, deny rules, allow
// rules, and mode defaults remain in one operator-policy boundary.

/// Asserts that the given path satisfies ADR-038 disk policy before operator
/// I/O proceeds.  The check delegates to [`disk_policy::enforce`] under the
/// process-wide policy mode so that `VEX_DISK_POLICY=strict` in CI rejects
/// any operator access outside `.vex/index/` and `.vex/state/`.
///
/// Operator read/write helpers call this with workspace-relative paths that
/// include the `.vex/` prefix when they target the durable search or task-state
/// layer.  Workspace-source reads (e.g. `read_file("src/main.rs")`) are
/// intentionally *not* routed here because those are user-requested file tool
/// operations that the model executes on behalf of the operator -- they touch
/// workspace content, not Vex's own durable state.
pub(crate) fn assert_durable_access(path: &Path) -> anyhow::Result<DiskPermission> {
    disk_policy::enforce(path, disk_policy::resolve_policy_mode())
}

/// Convenience check that returns `true` when the path falls under an allowed
/// durable layer (search index or task-state map) without raising errors in
/// any policy mode.
#[cfg(test)]
pub(crate) fn is_durable_path(path: &Path) -> bool {
    let permission = disk_policy::check_path(path);
    matches!(
        permission,
        DiskPermission::SearchIndex | DiskPermission::TaskStateMap
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn assert_durable_allows_index_path() {
        let p = PathBuf::from(".vex/index/chunks.bin");
        let perm = assert_durable_access(&p).expect("index path should be allowed");
        assert_eq!(perm, DiskPermission::SearchIndex);
    }

    #[test]
    fn assert_durable_allows_state_path() {
        let p = PathBuf::from(".vex/state/task-001.json");
        let perm = assert_durable_access(&p).expect("state path should be allowed");
        assert_eq!(perm, DiskPermission::TaskStateMap);
    }

    #[test]
    fn assert_durable_rejects_source_in_strict_mode() {
        let p = PathBuf::from("src/main.rs");
        let err = disk_policy::enforce(&p, disk_policy::DiskPolicyMode::Strict)
            .expect_err("strict mode must reject workspace source");
        assert!(err.to_string().contains("forbidden disk access"));
    }

    #[test]
    fn is_durable_path_classifies_correctly() {
        assert!(is_durable_path(Path::new(".vex/index/chunks.bin")));
        assert!(is_durable_path(Path::new(".vex/state/task-001.json")));
        assert!(!is_durable_path(Path::new("src/main.rs")));
        assert!(!is_durable_path(Path::new(".vex/config.toml")));
    }
}
