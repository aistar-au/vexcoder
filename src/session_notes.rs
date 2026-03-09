use crate::api::ApiClient;
use crate::config::Config;
use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn build_api_client_with_notes(config: &Config) -> Result<(ApiClient, Option<String>)> {
    let (notes_content, notes_warning) = resolve_notes_for_injection(config.notes_path.as_deref());
    let client = ApiClient::new(config)?.with_notes_content(notes_content);
    Ok((client, notes_warning))
}

fn memory_token_budget() -> usize {
    std::env::var("VEX_MAX_MEMORY_TOKENS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|budget| *budget > 0)
        .unwrap_or(2048)
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
    let fallback = PathBuf::from(".vex-memory.md");
    fallback.exists().then_some(fallback)
}

pub fn resolve_notes_path_for_write(explicit_path: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit_path {
        return path.to_path_buf();
    }
    if let Some(root) = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return PathBuf::from(root).join("vex").join("memory.md");
    }
    if let Some(home) = std::env::var("HOME").ok().filter(|value| !value.is_empty()) {
        return PathBuf::from(home)
            .join(".config")
            .join("vex")
            .join("memory.md");
    }
    PathBuf::from(".vex-memory.md")
}

pub fn clear_notes_file(explicit_path: Option<&Path>) -> std::io::Result<()> {
    let path = resolve_notes_path_for_read(explicit_path)
        .unwrap_or_else(|| resolve_notes_path_for_write(explicit_path));
    if path.exists() {
        std::fs::write(path, "")?;
    }
    Ok(())
}

pub fn resolve_notes_for_injection(
    explicit_path: Option<&Path>,
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

    let token_budget = memory_token_budget();
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
