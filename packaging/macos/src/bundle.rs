//! App bundle path resolution — ADR-024 Phase H PH-01.
//!
//! Resolves the path of the embedded vex binary at runtime.  In a correctly
//! assembled Vex.app bundle both `vex-launcher` and `vex` are placed at
//! `Contents/MacOS/`.  Outside a bundle the binary is located via `PATH`
//! so the launcher can be tested during development.

use anyhow::{bail, Result};
use std::path::PathBuf;

/// Returns the path to the vex binary that should be launched.
///
/// Resolution order:
///
/// 1. `<same directory as this launcher binary>/vex` — the standard in-bundle
///    layout (`Contents/MacOS/vex-launcher` and `Contents/MacOS/vex`).
/// 2. `vex` on `PATH` — development fallback when running outside a bundle.
///
/// An `Err` is returned only when neither location yields a regular file.
pub fn vex_binary_path() -> Result<PathBuf> {
    // Strategy 1: sibling binary in the same directory.
    let mut exe = std::env::current_exe().unwrap_or_default();
    exe.pop();
    let candidate = exe.join("vex");
    if candidate.is_file() {
        return Ok(candidate);
    }

    // Strategy 2: PATH lookup.
    if let Some(path) = find_on_path("vex") {
        return Ok(path);
    }

    bail!(
        "vex binary not found in app bundle ({}) or on PATH",
        candidate.display()
    )
}

/// Searches each directory in `PATH` for a file named `name`.
/// Returns the first match that is a regular file.
pub(crate) fn find_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::find_on_path;

    #[test]
    fn find_on_path_locates_system_binary() {
        // `sh` is universally available on macOS and Linux CI runners.
        assert!(
            find_on_path("sh").is_some(),
            "expected 'sh' to be discoverable on PATH"
        );
    }

    #[test]
    fn find_on_path_returns_none_for_absent_binary() {
        // A name that cannot plausibly exist as a real binary.
        assert!(
            find_on_path("vex-test-sentinel-absent-38f2a").is_none(),
            "expected sentinel name to be absent from PATH"
        );
    }

    #[test]
    fn find_on_path_returns_regular_file_not_directory() {
        // Verify the function never returns a directory entry.
        if let Some(path) = find_on_path("sh") {
            assert!(path.is_file(), "returned path is not a regular file: {path:?}");
        }
    }
}
