use crate::api::ApiClient;
use crate::config::Config;
use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn build_api_client_with_notes(config: &Config) -> Result<(ApiClient, Option<String>)> {
    let (notes_content, notes_warning) =
        resolve_notes_for_injection(config.notes_path.as_deref(), config.max_memory_tokens);
    let client = ApiClient::new(config)?.with_notes_content(notes_content);
    Ok((client, notes_warning))
}

pub fn resolve_notes_path_for_read(explicit_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit_path {
        return Some(path.to_path_buf());
    }
    if let Some(root) = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        let path = PathBuf::from(root).join("vex").join("memory.md");
        if path.exists() {
            return Some(path);
        }
    }
    if let Some(home) = std::env::var("HOME").ok().filter(|value| !value.is_empty()) {
        let xdg_path = PathBuf::from(&home)
            .join(".config")
            .join("vex")
            .join("memory.md");
        if xdg_path.exists() {
            return Some(xdg_path);
        }
        let legacy_path = PathBuf::from(home).join(".vex").join("memory.md");
        if legacy_path.exists() {
            return Some(legacy_path);
        }
    }
    None
}

pub fn resolve_notes_path_for_write(explicit_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit_path {
        return Some(path.to_path_buf());
    }
    if let Some(root) = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Some(PathBuf::from(root).join("vex").join("memory.md"));
    }
    if let Some(home) = std::env::var("HOME").ok().filter(|value| !value.is_empty()) {
        return Some(
            PathBuf::from(home)
                .join(".config")
                .join("vex")
                .join("memory.md"),
        );
    }
    None
}

pub fn clear_notes_file(explicit_path: Option<&Path>) -> std::io::Result<()> {
    let path = resolve_notes_path_for_read(explicit_path)
        .or_else(|| resolve_notes_path_for_write(explicit_path));
    if let Some(path) = path.filter(|path| path.exists()) {
        std::fs::write(path, "")?;
    }
    Ok(())
}

pub fn resolve_notes_for_injection(
    explicit_path: Option<&Path>,
    token_budget: usize,
) -> (Option<String>, Option<String>) {
    let Some(path) = resolve_notes_path_for_read(explicit_path) else {
        return (None, None);
    };
    let Ok(content) = std::fs::read_to_string(path) else {
        return (None, None);
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return (None, None);
    }

    let estimated_tokens = trimmed.len().saturating_add(3) / 4;
    if estimated_tokens > token_budget {
        return (
            None,
            Some(format!(
                "[memory] notes exceed token budget ({estimated_tokens} > {token_budget}), skipped"
            )),
        );
    }

    (Some(trimmed.to_string()), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_notes_path_for_read_rejects_repo_local_fallback() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        let old_home = std::env::var("HOME").ok();
        let old_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        let old_cwd = std::env::current_dir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("HOME", &home);
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::set_current_dir(temp.path()).unwrap();
        std::fs::write(temp.path().join(".vex-memory.md"), "repo local note\n").unwrap();

        let resolved = resolve_notes_path_for_read(None);

        std::env::set_current_dir(old_cwd).unwrap();
        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match old_xdg {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }

        assert_eq!(resolved, None);
    }

    #[test]
    fn test_resolve_notes_for_injection_uses_passed_budget() {
        let temp = tempfile::tempdir().unwrap();
        let notes_path = temp.path().join("memory.md");
        std::fs::write(&notes_path, "12345").unwrap();

        let (content, warning) = resolve_notes_for_injection(Some(notes_path.as_path()), 1);

        assert!(content.is_none());
        assert!(warning
            .as_deref()
            .is_some_and(|message| message.contains("notes exceed token budget")));
    }
}
