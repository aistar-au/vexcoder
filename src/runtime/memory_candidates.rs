//! Typed, reviewable memory candidates (ADR-051 Batch 2).
//!
//! The JSON sidecar next to the notes file is the source of truth. The
//! markdown notes file is rewritten as a projection so existing `/memory`
//! readers still see the same path. Notes live under the user config
//! directory, so this path is not gated by durable-access policy.

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::runtime::token_count::token_count;

pub const MEMORY_CANDIDATES_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSource {
    User,
    Feedback,
    Project,
    Reference,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    Pending,
    Accepted,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MemoryCandidate {
    pub source: CandidateSource,
    pub topic: String,
    pub body: String,
    pub status: CandidateStatus,
    pub source_reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct MemoryCandidateStore {
    pub schema_version: u32,
    pub candidates: Vec<MemoryCandidate>,
}

impl Default for MemoryCandidateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryCandidateStore {
    pub fn new() -> Self {
        Self {
            schema_version: MEMORY_CANDIDATES_SCHEMA_VERSION,
            candidates: Vec::new(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        crate::util::write_json_safe(path, self, "memory candidates")?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read memory candidates: {}", path.display()))?;
        let store: Self = serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to deserialize memory candidates: {}",
                path.display()
            )
        })?;
        Ok(store)
    }
}

pub fn candidates_path(notes_path: &Path) -> PathBuf {
    notes_path.with_extension("candidates.json")
}

pub fn load_or_migrate(notes_path: &Path) -> Result<MemoryCandidateStore> {
    let json_path = candidates_path(notes_path);
    if json_path.exists() {
        return MemoryCandidateStore::load(&json_path);
    }
    let mut store = MemoryCandidateStore::new();
    if notes_path.exists() {
        let content = std::fs::read_to_string(notes_path)?;
        store.candidates = migrate_markdown_lines(&content);
        if !store.candidates.is_empty() {
            persist_store(notes_path, &store)?;
        }
    }
    Ok(store)
}

pub fn persist_store(notes_path: &Path, store: &MemoryCandidateStore) -> Result<()> {
    if let Some(parent) = notes_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    store.save(&candidates_path(notes_path))?;
    rewrite_notes_markdown(notes_path, store)?;
    Ok(())
}

pub fn add_user_note(notes_path: &Path, note: &str) -> Result<()> {
    let mut store = load_or_migrate(notes_path)?;
    store.candidates.push(MemoryCandidate {
        source: CandidateSource::User,
        topic: topic_from(note),
        body: note.to_string(),
        status: CandidateStatus::Accepted,
        source_reference: "/memory add".to_string(),
    });
    persist_store(notes_path, &store)
}

pub fn append_feedback_notes(notes_path: &Path, formatted_auto_lines: &[String]) -> Result<()> {
    let mut store = load_or_migrate(notes_path)?;
    for line in formatted_auto_lines {
        let body = strip_auto_note_body(line);
        let timestamp = parse_auto_timestamp(line);
        store.candidates.push(MemoryCandidate {
            source: CandidateSource::Feedback,
            topic: topic_from(&body),
            body,
            status: CandidateStatus::Pending,
            source_reference: format!("auto:{timestamp}"),
        });
    }
    persist_store(notes_path, &store)
}

pub fn clear_store(notes_path: &Path) -> Result<()> {
    persist_store(notes_path, &MemoryCandidateStore::new())
}

pub fn remove_feedback_candidates(notes_path: &Path) -> Result<usize> {
    if !notes_path.exists() && !candidates_path(notes_path).exists() {
        return Ok(0);
    }
    let mut store = load_or_migrate(notes_path)?;
    let before = store.candidates.len();
    store
        .candidates
        .retain(|candidate| candidate.source != CandidateSource::Feedback);
    let removed = before - store.candidates.len();
    persist_store(notes_path, &store)?;
    Ok(removed)
}

pub fn accept_pending(notes_path: &Path, selector: &str) -> Result<Option<String>> {
    let mut store = load_or_migrate(notes_path)?;
    let pending_indices: Vec<usize> = store
        .candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.status == CandidateStatus::Pending)
        .map(|(index, _)| index)
        .collect();
    if pending_indices.is_empty() {
        return Ok(None);
    }
    let target = if let Ok(number) = selector.parse::<usize>() {
        if number == 0 || number > pending_indices.len() {
            return Ok(None);
        }
        pending_indices[number - 1]
    } else {
        let needle = selector.to_ascii_lowercase();
        match pending_indices.into_iter().find(|&index| {
            store.candidates[index]
                .topic
                .to_ascii_lowercase()
                .contains(&needle)
                || store.candidates[index]
                    .body
                    .to_ascii_lowercase()
                    .contains(&needle)
        }) {
            Some(index) => index,
            None => return Ok(None),
        }
    };
    store.candidates[target].status = CandidateStatus::Accepted;
    let body = store.candidates[target].body.clone();
    persist_store(notes_path, &store)?;
    Ok(Some(body))
}

/// Inject only `Accepted` candidates, dropping the lowest-priority accepted
/// item until the remaining set fits `token_budget`.
pub fn inject_accepted(
    store: &MemoryCandidateStore,
    token_budget: usize,
) -> (Option<String>, Option<String>) {
    let mut selected: Vec<(&MemoryCandidate, usize)> = store
        .candidates
        .iter()
        .filter(|candidate| candidate.status == CandidateStatus::Accepted)
        .map(|candidate| (candidate, token_count(&candidate.body)))
        .collect();
    selected.sort_by_key(|(candidate, _)| source_priority(candidate.source));

    let mut total: usize = selected.iter().map(|(_, tokens)| *tokens).sum();
    let mut dropped = 0usize;
    while total > token_budget {
        let Some((_, tokens)) = selected.pop() else {
            break;
        };
        total = total.saturating_sub(tokens);
        dropped += 1;
    }

    if selected.is_empty() {
        if dropped > 0 {
            return (
                None,
                Some(format!(
                    "[memory] notes exceed token budget (dropped {dropped} accepted candidate(s) above {token_budget})"
                )),
            );
        }
        return (None, None);
    }

    let content = selected
        .iter()
        .map(|(candidate, _)| candidate.body.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let warning = if dropped > 0 {
        Some(format!(
            "[memory] dropped {dropped} accepted candidate(s) over token budget ({token_budget})"
        ))
    } else {
        None
    };
    (Some(content), warning)
}

fn source_priority(source: CandidateSource) -> u8 {
    match source {
        CandidateSource::User => 0,
        CandidateSource::Feedback => 1,
        CandidateSource::Project => 2,
        CandidateSource::Reference => 3,
    }
}

fn topic_from(body: &str) -> String {
    let trimmed = body.trim();
    const MAX_CHARS: usize = 48;
    if trimmed.chars().count() <= MAX_CHARS {
        trimmed.to_string()
    } else {
        let truncated: String = trimmed.chars().take(MAX_CHARS).collect();
        format!("{truncated}...")
    }
}

pub fn migrate_markdown_lines(content: &str) -> Vec<MemoryCandidate> {
    let mut out = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if crate::auto_memory::is_auto_note_line(trimmed) {
            let body = strip_auto_note_body(trimmed);
            let timestamp = parse_auto_timestamp(trimmed);
            out.push(MemoryCandidate {
                source: CandidateSource::Feedback,
                topic: topic_from(&body),
                body,
                status: CandidateStatus::Pending,
                source_reference: format!("auto:{timestamp}"),
            });
        } else {
            out.push(MemoryCandidate {
                source: CandidateSource::User,
                topic: topic_from(trimmed),
                body: trimmed.to_string(),
                status: CandidateStatus::Accepted,
                source_reference: "memory.md".to_string(),
            });
        }
    }
    out
}

fn strip_auto_note_body(line: &str) -> String {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix('[')
        && let Some((_, suffix)) = rest.split_once("] ")
        && let Some(body) = suffix.strip_prefix("[auto] ")
    {
        return body.to_string();
    }
    trimmed.to_string()
}

fn parse_auto_timestamp(line: &str) -> String {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix('[')
        && let Some((timestamp, _)) = rest.split_once(']')
        && !timestamp.is_empty()
        && timestamp.chars().all(|ch| ch.is_ascii_digit())
    {
        return timestamp.to_string();
    }
    "0".to_string()
}

fn rewrite_notes_markdown(notes_path: &Path, store: &MemoryCandidateStore) -> Result<()> {
    let rendered = render_notes_markdown(&store.candidates);
    std::fs::write(notes_path, rendered)
        .with_context(|| format!("Failed to write notes projection: {}", notes_path.display()))?;
    Ok(())
}

fn render_notes_markdown(candidates: &[MemoryCandidate]) -> String {
    if candidates.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for candidate in candidates {
        match candidate.source {
            CandidateSource::Feedback => {
                if let Some(timestamp) = candidate.source_reference.strip_prefix("auto:") {
                    out.push_str(&format!("[{timestamp}] [auto] {}\n", candidate.body));
                } else {
                    out.push_str(&format!("[0] [auto] {}\n", candidate.body));
                }
            }
            _ => {
                out.push_str(&candidate.body);
                out.push('\n');
            }
        }
    }
    out
}

/// Pretty JSON Schema generated from the store type. Used by the drift test.
#[cfg(test)]
pub fn memory_candidates_json_schema_pretty() -> String {
    let schema = schemars::schema_for!(MemoryCandidateStore);
    let mut json = serde_json::to_string_pretty(&schema).expect("schema serializes");
    if !json.ends_with('\n') {
        json.push('\n');
    }
    json
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_candidate(
        source: CandidateSource,
        body: &str,
        status: CandidateStatus,
    ) -> MemoryCandidate {
        MemoryCandidate {
            source,
            topic: topic_from(body),
            body: body.to_string(),
            status,
            source_reference: "test".to_string(),
        }
    }

    #[test]
    fn pending_memory_candidates_are_not_injected() {
        let mut store = MemoryCandidateStore::new();
        store.candidates.push(sample_candidate(
            CandidateSource::User,
            "keep this accepted fact",
            CandidateStatus::Accepted,
        ));
        store.candidates.push(sample_candidate(
            CandidateSource::Feedback,
            "extracted pending fact must stay out of the prompt",
            CandidateStatus::Pending,
        ));

        let (content, warning) = inject_accepted(&store, 4096);
        let content = content.expect("accepted candidate should inject");
        assert!(content.contains("keep this accepted fact"));
        assert!(!content.contains("extracted pending fact"));
        assert!(warning.is_none());
    }

    #[test]
    fn inject_accepted_drops_lowest_priority_when_over_budget() {
        let mut store = MemoryCandidateStore::new();
        store.candidates.push(sample_candidate(
            CandidateSource::User,
            "user-priority-alpha",
            CandidateStatus::Accepted,
        ));
        store.candidates.push(sample_candidate(
            CandidateSource::Reference,
            "reference-priority-omega extra distinct tokens here",
            CandidateStatus::Accepted,
        ));
        let user_tokens = token_count("user-priority-alpha");
        let (content, warning) = inject_accepted(&store, user_tokens);
        let content = content.expect("user candidate should remain");
        assert_eq!(content, "user-priority-alpha");
        assert!(warning.is_some());
    }

    #[test]
    fn migrate_markdown_lines_marks_auto_as_pending_feedback() {
        let lines = "[42] [auto] extracted convention\nmanual operator note\n";
        let candidates = migrate_markdown_lines(lines);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].source, CandidateSource::Feedback);
        assert_eq!(candidates[0].status, CandidateStatus::Pending);
        assert_eq!(candidates[0].body, "extracted convention");
        assert_eq!(candidates[1].source, CandidateSource::User);
        assert_eq!(candidates[1].status, CandidateStatus::Accepted);
        assert_eq!(candidates[1].body, "manual operator note");
    }

    #[test]
    fn add_user_note_dual_writes_json_and_markdown() {
        let dir = TempDir::new().unwrap();
        let notes = dir.path().join("memory.md");
        add_user_note(&notes, "track the open build issue").unwrap();
        let markdown = std::fs::read_to_string(&notes).unwrap();
        assert!(markdown.contains("track the open build issue"));
        let store = MemoryCandidateStore::load(&candidates_path(&notes)).unwrap();
        assert_eq!(store.candidates.len(), 1);
        assert_eq!(store.candidates[0].source, CandidateSource::User);
        assert_eq!(store.candidates[0].status, CandidateStatus::Accepted);
    }

    #[test]
    fn accept_pending_promotes_feedback_candidate() {
        let dir = TempDir::new().unwrap();
        let notes = dir.path().join("memory.md");
        append_feedback_notes(&notes, &["[7] [auto] extracted fact".to_string()]).unwrap();
        let accepted = accept_pending(&notes, "1").unwrap();
        assert_eq!(accepted.as_deref(), Some("extracted fact"));
        let store = load_or_migrate(&notes).unwrap();
        assert_eq!(store.candidates[0].status, CandidateStatus::Accepted);
        let (content, _) = inject_accepted(&store, 4096);
        assert_eq!(content.as_deref(), Some("extracted fact"));
    }

    #[test]
    fn memory_candidates_schema_matches_checked_in_file() {
        let generated = memory_candidates_json_schema_pretty();
        let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("schemas/memory_candidates.schema.json");
        if std::env::var_os("UPDATE_MEMORY_CANDIDATES_SCHEMA").is_some() {
            std::fs::write(&schema_path, &generated).expect("write generated schema");
        }
        let expected = include_str!("../../schemas/memory_candidates.schema.json");
        assert_eq!(
            expected, generated,
            "schemas/memory_candidates.schema.json drifted from schema_for!(MemoryCandidateStore); rerun with UPDATE_MEMORY_CANDIDATES_SCHEMA=1"
        );
    }
}
