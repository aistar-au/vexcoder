use std::path::Path;

use crate::disk_policy::{self, DiskPermission};

pub(crate) fn assert_durable_access(path: &Path) -> anyhow::Result<DiskPermission> {
    disk_policy::enforce(path, disk_policy::resolve_policy_mode())
}

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
