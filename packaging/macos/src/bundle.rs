use anyhow::{bail, Result};
use std::path::PathBuf;

pub fn vex_binary_path() -> Result<PathBuf> {
    
    let mut exe = std::env::current_exe().unwrap_or_default();
    exe.pop();
    let candidate = exe.join("vex");
    if candidate.is_file() {
        return Ok(candidate);
    }

    if let Some(path) = find_on_path("vex") {
        return Ok(path);
    }

    bail!(
        "vex binary not found in app bundle ({}) or on PATH",
        candidate.display()
    )
}

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
        
        assert!(
            find_on_path("sh").is_some(),
            "expected 'sh' to be discoverable on PATH"
        );
    }

    #[test]
    fn find_on_path_returns_none_for_absent_binary() {
        
        assert!(
            find_on_path("vex-test-sentinel-absent-38f2a").is_none(),
            "expected sentinel name to be absent from PATH"
        );
    }

    #[test]
    fn find_on_path_returns_regular_file_not_directory() {
        
        if let Some(path) = find_on_path("sh") {
            assert!(path.is_file(), "returned path is not a regular file: {path:?}");
        }
    }
}
