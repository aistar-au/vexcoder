use anyhow::{anyhow, Result};
use std::path::Path;

/// Categorizes filesystem access per ADR-038 Memory-First Architecture.
///
/// Persistent disk I/O is permitted only for search index and task state.
/// Everything else (config, file snapshots, git summaries, conversation
/// history) must be RAM-resident after first load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskPermission {
    /// Codebase search/index persistence (`.vex/index/`).
    SearchIndex,
    /// Task state map JSONs (`.vex/state/*.json`).
    TaskStateMap,
    /// Path does not match any permitted disk-I/O category.
    Forbidden,
}

/// Controls how forbidden disk access is handled at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskPolicyMode {
    /// No enforcement (default).
    Off,
    /// Log a warning on forbidden access.
    Warn,
    /// Panic on forbidden access (CI gate via `VEX_DISK_POLICY=strict`).
    Strict,
}

/// Enforce the ADR-038 disk policy for a path under an explicit mode.
pub fn enforce(path: &Path, mode: DiskPolicyMode) -> Result<DiskPermission> {
    let permission = check_path(path);
    if permission != DiskPermission::Forbidden {
        return Ok(permission);
    }

    let message = format!(
        "forbidden disk access outside .vex/index or .vex/state: {}",
        path.display()
    );

    match mode {
        DiskPolicyMode::Off => Ok(permission),
        DiskPolicyMode::Warn => {
            eprintln!("[disk_policy] {message}");
            Ok(permission)
        }
        DiskPolicyMode::Strict => Err(anyhow!(message)),
    }
}

/// Enforce the ADR-038 disk policy for a path using the current process mode.
pub fn enforce_runtime(path: &Path) -> Result<DiskPermission> {
    enforce(path, resolve_policy_mode())
}

/// Classify a path according to the ADR-038 disk permission model.
pub fn check_path(path: &Path) -> DiskPermission {
    // Fast path: component-based match (works when the OS treats the separator
    // natively, i.e. backslash on Windows, forward-slash everywhere).
    for component in path.components() {
        let segment = component.as_os_str().to_string_lossy();
        if segment == ".vex" {
            return classify_vex_subpath(path);
        }
    }
    // Fallback: string-based match handles paths that embed the non-native
    // separator (e.g. a Windows-style `.vex\index\…` string processed on Unix).
    let lossy = path.to_string_lossy();
    if lossy.contains(".vex\\") || lossy.contains(".vex/") {
        return classify_vex_subpath(path);
    }
    DiskPermission::Forbidden
}

fn classify_vex_subpath(path: &Path) -> DiskPermission {
    let lossy = path.to_string_lossy();
    let normalized = lossy.replace('\\', "/");
    if let Some(after_vex) = normalized.split(".vex/").nth(1) {
        if after_vex.starts_with("index/") || after_vex == "index" {
            return DiskPermission::SearchIndex;
        }
        if after_vex.starts_with("state/") || after_vex == "state" {
            return DiskPermission::TaskStateMap;
        }
    }
    DiskPermission::Forbidden
}

/// Resolve the disk policy mode from the `VEX_DISK_POLICY` environment variable.
pub fn resolve_policy_mode() -> DiskPolicyMode {
    match std::env::var("VEX_DISK_POLICY") {
        Ok(val) => match val.to_lowercase().as_str() {
            "strict" => DiskPolicyMode::Strict,
            "warn" => DiskPolicyMode::Warn,
            "off" | "" => DiskPolicyMode::Off,
            other => {
                eprintln!("[disk_policy] unknown VEX_DISK_POLICY={other:?}; defaulting to off");
                DiskPolicyMode::Off
            }
        },
        Err(_) => DiskPolicyMode::Off,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn index_directory_is_search_index() {
        let p = PathBuf::from("/home/user/project/.vex/index/chunks.bin");
        assert_eq!(check_path(&p), DiskPermission::SearchIndex);
    }

    #[test]
    fn index_root_is_search_index() {
        let p = PathBuf::from(".vex/index");
        assert_eq!(check_path(&p), DiskPermission::SearchIndex);
    }

    #[test]
    fn state_json_is_task_state_map() {
        let p = PathBuf::from("/repo/.vex/state/task-001.json");
        assert_eq!(check_path(&p), DiskPermission::TaskStateMap);
    }

    #[test]
    fn state_subdirectory_is_task_state_map() {
        let p = PathBuf::from(".vex/state/worktrees/task-001");
        assert_eq!(check_path(&p), DiskPermission::TaskStateMap);
    }

    #[test]
    fn state_root_is_task_state_map() {
        let p = PathBuf::from("/repo/.vex/state");
        assert_eq!(check_path(&p), DiskPermission::TaskStateMap);
    }

    #[test]
    fn config_toml_is_forbidden() {
        let p = PathBuf::from("/repo/.vex/config.toml");
        assert_eq!(check_path(&p), DiskPermission::Forbidden);
    }

    #[test]
    fn source_file_is_forbidden() {
        let p = PathBuf::from("/repo/src/main.rs");
        assert_eq!(check_path(&p), DiskPermission::Forbidden);
    }

    #[test]
    fn relative_source_is_forbidden() {
        let p = PathBuf::from("src/lib.rs");
        assert_eq!(check_path(&p), DiskPermission::Forbidden);
    }

    #[test]
    fn policy_mode_defaults_to_off() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::remove_var("VEX_DISK_POLICY");
        assert_eq!(resolve_policy_mode(), DiskPolicyMode::Off);
    }

    #[test]
    fn unknown_policy_mode_defaults_to_off() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var("VEX_DISK_POLICY", "mystery");
        assert_eq!(resolve_policy_mode(), DiskPolicyMode::Off);
        std::env::remove_var("VEX_DISK_POLICY");
    }

    #[test]
    fn enforce_strict_rejects_forbidden_paths() {
        let error = enforce(Path::new("src/main.rs"), DiskPolicyMode::Strict)
            .expect_err("strict mode must reject workspace source access");
        assert!(error.to_string().contains("forbidden disk access"));
    }

    #[test]
    fn enforce_strict_allows_index_paths() {
        let permission = enforce(Path::new(".vex/index/chunks.bin"), DiskPolicyMode::Strict)
            .expect("strict mode should allow search index access");
        assert_eq!(permission, DiskPermission::SearchIndex);
    }

    #[test]
    fn index_prefix_without_separator_is_forbidden() {
        // Regression: ".vex/indexing.txt" must NOT match SearchIndex.
        assert_eq!(
            check_path(&PathBuf::from(".vex/indexing.txt")),
            DiskPermission::Forbidden,
        );
        assert_eq!(
            check_path(&PathBuf::from("/repo/.vex/indexed-data")),
            DiskPermission::Forbidden,
        );
    }

    #[test]
    fn state_prefix_without_separator_is_forbidden() {
        // Symmetric check: ".vex/stateful.bin" must NOT match TaskStateMap.
        assert_eq!(
            check_path(&PathBuf::from(".vex/stateful.bin")),
            DiskPermission::Forbidden,
        );
        assert_eq!(
            check_path(&PathBuf::from("/repo/.vex/statefiles")),
            DiskPermission::Forbidden,
        );
    }
}
