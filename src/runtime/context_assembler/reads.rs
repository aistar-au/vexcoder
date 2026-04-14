use anyhow::Result;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::runtime::context_cache::CachedFileRead;
use crate::runtime::text_util::truncate_head_bytes;

use super::FileRollup;

const STANDALONE_PATH_EXTENSIONS: &[&str] = &["rs", "toml", "md", "txt", "json", "sh"];

pub(super) fn extract_candidate_paths(instruction: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for token in instruction.split_whitespace() {
        let candidate = token.trim_matches(|c: char| {
            matches!(
                c,
                '"' | '\'' | '`' | ',' | '.' | ';' | ':' | '(' | ')' | '[' | ']' | '{' | '}'
            )
        });
        if candidate.is_empty() {
            continue;
        }
        if candidate.starts_with('/') || candidate.starts_with('-') {
            continue;
        }
        if !(candidate.contains('/') || candidate.contains('.')) {
            continue;
        }

        let normalized = candidate.trim_start_matches("./").to_string();
        if normalized.is_empty() {
            continue;
        }
        if normalized
            .chars()
            .next()
            .is_some_and(|value| value.is_ascii_digit())
        {
            continue;
        }
        if !normalized.contains('/') {
            let Some(extension) = Path::new(&normalized)
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase())
            else {
                continue;
            };
            if !STANDALONE_PATH_EXTENSIONS.contains(&extension.as_str()) {
                continue;
            }
        }
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }

    out
}

pub(super) fn rollup_from_read(
    path: PathBuf,
    result: Result<CachedFileRead>,
    max_file_bytes: usize,
) -> (FileRollup, bool) {
    match result {
        Ok(read) => {
            let (content, content_limited) = truncate_head_bytes(&read.content, max_file_bytes);
            (
                FileRollup {
                    path,
                    content: Some(content),
                    content_limited,
                },
                read.cache_hit,
            )
        }
        Err(_) => (
            FileRollup {
                path,
                content: None,
                content_limited: false,
            },
            false,
        ),
    }
}

pub(super) fn infer_related_path_candidates(content: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(path) = infer_rust_use_path(trimmed)
            && seen.insert(path.clone())
        {
            out.push(path);
        }

        if let Some(path) = infer_python_import_path(trimmed)
            && seen.insert(path.clone())
        {
            out.push(path);
        }

        if let Some(path) = infer_js_import_path(trimmed)
            && seen.insert(path.clone())
        {
            out.push(path);
        }
    }

    out
}

fn infer_rust_use_path(line: &str) -> Option<PathBuf> {
    let value = line
        .strip_prefix("use ")
        .or_else(|| line.strip_prefix("pub use "))?;
    let mut module = value
        .split("//")
        .next()?
        .trim()
        .trim_end_matches(';')
        .trim();
    if module.is_empty() {
        return None;
    }
    if let Some((prefix, _)) = module.split_once(" as ") {
        module = prefix.trim();
    }
    if let Some((prefix, _)) = module.split_once('{') {
        module = prefix.trim();
    }
    module = module.trim_end_matches(':').trim_end_matches(':').trim();

    let relative = module
        .strip_prefix("crate::")
        .or_else(|| module.strip_prefix("super::"))
        .or_else(|| module.strip_prefix("self::"))?;
    if relative.is_empty() {
        return None;
    }
    let path = relative.replace("::", "/");
    Some(PathBuf::from("src").join(format!("{path}.rs")))
}

fn infer_python_import_path(line: &str) -> Option<PathBuf> {
    let module = if let Some(value) = line.strip_prefix("from ") {
        let (module, _) = value.split_once(" import ")?;
        module.trim()
    } else if let Some(value) = line.strip_prefix("import ") {
        if value.contains(" from ") || value.contains('"') || value.contains('\'') {
            return None;
        }
        value
            .split(',')
            .next()
            .and_then(|entry| entry.split_whitespace().next())?
            .trim()
    } else {
        return None;
    };

    if module.is_empty() || module.starts_with('.') {
        return None;
    }
    let path = module.replace('.', "/");
    Some(PathBuf::from(format!("{path}.py")))
}

fn infer_js_import_path(line: &str) -> Option<PathBuf> {
    if !line.starts_with("import ") && !line.starts_with("export ") {
        return None;
    }
    let specifier = extract_quoted_specifier(line)?;
    if specifier.is_empty() || specifier.starts_with('/') || specifier.starts_with("../") {
        return None;
    }

    let normalized = specifier.trim_start_matches("./");
    if normalized.is_empty() {
        return None;
    }
    if Path::new(normalized).extension().is_some() {
        return Some(PathBuf::from(normalized));
    }
    Some(PathBuf::from(format!("{normalized}.js")))
}

fn extract_quoted_specifier(line: &str) -> Option<&str> {
    for quote in ['"', '\''] {
        if let Some(start) = line.find(quote) {
            let suffix = &line[start + 1..];
            if let Some(end) = suffix.find(quote) {
                return Some(&suffix[..end]);
            }
        }
    }
    None
}
