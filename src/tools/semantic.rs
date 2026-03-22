use crate::tools::embed::{embed_texts, EmbeddingConfig};
use crate::tools::index::IndexChunk;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tempfile::Builder;
use tokio::fs;
use tokio::io::AsyncWriteExt;

const SEMANTIC_INDEX_VERSION: u32 = 1;
const DEFAULT_INDEX_MAX_FILES: usize = 5_000;
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

#[derive(Debug, Clone)]
pub struct SemanticChunkScore {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub name: String,
    pub score: f64,
}

pub async fn semantic_search(
    workspace_root: &Path,
    index: &[IndexChunk],
    query: &str,
    config: &EmbeddingConfig,
    max_results: Option<usize>,
) -> Result<Vec<SemanticChunkScore>> {
    if query.trim().is_empty() || index.is_empty() {
        return Ok(Vec::new());
    }

    let selected_chunks = eligible_chunks(index);
    if selected_chunks.is_empty() {
        return Ok(Vec::new());
    }

    let mut persisted = load_index(workspace_root, config).await?;
    let mut stored_by_key: HashMap<String, PersistedSemanticChunk> = persisted
        .chunks
        .drain(..)
        .map(|chunk| (chunk.chunk_key.clone(), chunk))
        .collect();
    let mut index_dirty = false;

    let valid_keys: HashSet<String> = selected_chunks
        .iter()
        .map(|chunk| chunk_key(chunk))
        .collect();
    let original_len = stored_by_key.len();
    stored_by_key.retain(|key, _| valid_keys.contains(key));
    index_dirty |= stored_by_key.len() != original_len;

    let mut missing_texts = Vec::new();
    let mut missing_chunks = Vec::new();
    for chunk in &selected_chunks {
        let key = chunk_key(chunk);
        let content_hash = source_hash(&chunk.source);
        let stale = stored_by_key
            .get(&key)
            .map(|stored| stored.content_hash != content_hash)
            .unwrap_or(true);
        if stale {
            missing_texts.push(chunk.source.clone());
            missing_chunks.push((key, chunk, content_hash));
        }
    }

    if !missing_texts.is_empty() {
        let embeddings = embed_texts(config, &missing_texts).await?;
        for ((key, chunk, content_hash), embedding) in missing_chunks.into_iter().zip(embeddings) {
            stored_by_key.insert(
                key.clone(),
                PersistedSemanticChunk {
                    chunk_key: key,
                    path: chunk.path.clone(),
                    start_line: chunk.start_line,
                    end_line: chunk.end_line,
                    name: chunk.name.clone(),
                    content_hash,
                    embedding,
                },
            );
        }
        index_dirty = true;
    }

    if index_dirty {
        let mut saved_chunks: Vec<PersistedSemanticChunk> =
            stored_by_key.values().cloned().collect();
        saved_chunks.sort_by(|left, right| left.chunk_key.cmp(&right.chunk_key));
        persisted.chunks = saved_chunks;
        save_index(workspace_root, &persisted).await?;
    }

    let mut query_embeddings = embed_texts(config, &[query.to_string()]).await?;
    let Some(query_embedding) = query_embeddings.pop() else {
        return Ok(Vec::new());
    };

    let cap = max_results.unwrap_or(10).max(1) * 3;
    let mut scores = Vec::new();
    for chunk in stored_by_key.values() {
        let Some(score) = cosine_similarity(&query_embedding, &chunk.embedding) else {
            continue;
        };
        scores.push(SemanticChunkScore {
            path: chunk.path.clone(),
            start_line: chunk.start_line,
            end_line: chunk.end_line,
            name: chunk.name.clone(),
            score,
        });
    }

    scores.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.start_line.cmp(&right.start_line))
    });
    scores.truncate(cap);
    Ok(scores)
}

fn eligible_chunks(index: &[IndexChunk]) -> Vec<&IndexChunk> {
    let max_files = std::env::var("VEX_INDEX_MAX_FILES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_INDEX_MAX_FILES);
    let mut allowed_paths = HashSet::new();
    let mut chunks = Vec::new();
    for chunk in index {
        if allowed_paths.len() >= max_files && !allowed_paths.contains(&chunk.path) {
            continue;
        }
        allowed_paths.insert(chunk.path.clone());
        chunks.push(chunk);
    }
    chunks
}

fn semantic_index_path(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(".vex")
        .join("index")
        .join("semantic-codebase-index.json")
}

async fn load_index(
    workspace_root: &Path,
    config: &EmbeddingConfig,
) -> Result<PersistedSemanticIndex> {
    let path = semantic_index_path(workspace_root);
    let fresh = || PersistedSemanticIndex {
        version: SEMANTIC_INDEX_VERSION,
        provider: config.provider.as_str().to_string(),
        model: config.model.clone(),
        chunks: Vec::new(),
    };

    let raw = match fs::read_to_string(&path).await {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(fresh()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read semantic index {}", path.display()));
        }
    };
    let parsed: PersistedSemanticIndex = match serde_json::from_str(&raw) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(fresh()),
    };
    if parsed.version != SEMANTIC_INDEX_VERSION
        || parsed.provider != config.provider.as_str()
        || parsed.model != config.model
    {
        return Ok(fresh());
    }
    Ok(parsed)
}

async fn save_index(workspace_root: &Path, persisted: &PersistedSemanticIndex) -> Result<()> {
    let path = semantic_index_path(workspace_root);
    let parent = path.parent().with_context(|| {
        format!(
            "failed to derive semantic index parent directory for {}",
            path.display()
        )
    })?;
    fs::create_dir_all(parent).await.with_context(|| {
        format!(
            "failed to create semantic index directory {}",
            parent.display()
        )
    })?;
    let raw = serde_json::to_vec_pretty(persisted).context("failed to encode semantic index")?;
    let temp_path = Builder::new()
        .prefix("semantic-codebase-index.")
        .suffix(".tmp")
        .tempfile_in(parent)
        .with_context(|| {
            format!(
                "failed to create temp semantic index in {}",
                parent.display()
            )
        })?
        .into_temp_path();
    let temp_path_buf = temp_path.to_path_buf();
    let mut file = fs::File::create(&temp_path_buf).await.with_context(|| {
        format!(
            "failed to create temp semantic index {}",
            temp_path_buf.display()
        )
    })?;
    file.write_all(&raw).await.with_context(|| {
        format!(
            "failed to write temp semantic index {}",
            temp_path_buf.display()
        )
    })?;
    file.sync_all().await.with_context(|| {
        format!(
            "failed to flush temp semantic index {}",
            temp_path_buf.display()
        )
    })?;
    drop(file);

    fs::rename(&temp_path_buf, &path)
        .await
        .with_context(|| format!("failed to replace semantic index {}", path.display()))?;
    Ok(())
}

fn chunk_key(chunk: &IndexChunk) -> String {
    format!(
        "{}:{}:{}:{}",
        chunk.path, chunk.start_line, chunk.end_line, chunk.name
    )
}

fn source_hash(source: &str) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in source.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f64> {
    if left.is_empty() || right.is_empty() || left.len() != right.len() {
        return None;
    }

    let mut dot = 0.0_f64;
    let mut left_norm = 0.0_f64;
    let mut right_norm = 0.0_f64;
    for (left_value, right_value) in left.iter().zip(right.iter()) {
        let left_value = f64::from(*left_value);
        let right_value = f64::from(*right_value);
        dot += left_value * right_value;
        left_norm += left_value * left_value;
        right_norm += right_value * right_value;
    }

    let denominator = left_norm.sqrt() * right_norm.sqrt();
    if denominator <= f64::EPSILON {
        return None;
    }
    Some(dot / denominator)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSemanticChunk {
    chunk_key: String,
    path: String,
    start_line: usize,
    end_line: usize,
    name: String,
    content_hash: u64,
    embedding: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSemanticIndex {
    version: u32,
    provider: String,
    model: String,
    chunks: Vec<PersistedSemanticChunk>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::index::ItemKind;
    use tempfile::tempdir;

    fn make_chunk(path: &str, name: &str, source: &str) -> IndexChunk {
        IndexChunk {
            path: path.to_string(),
            start_line: 1,
            end_line: 5,
            kind: ItemKind::Function,
            name: name.to_string(),
            parent_scope: None,
            source: source.to_string(),
        }
    }

    #[test]
    fn test_eligible_chunks_respects_file_limit() {
        let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
        std::env::set_var("VEX_INDEX_MAX_FILES", "1");

        let chunks = vec![
            make_chunk("src/one.rs", "one", "fn one() {}"),
            make_chunk("src/two.rs", "two", "fn two() {}"),
        ];
        let selected = eligible_chunks(&chunks);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].path, "src/one.rs");

        std::env::remove_var("VEX_INDEX_MAX_FILES");
    }

    #[test]
    fn test_cosine_similarity_handles_dimension_mismatch() {
        assert!(cosine_similarity(&[1.0, 2.0], &[1.0]).is_none());
    }

    #[test]
    fn test_source_hash_is_stable() {
        assert_eq!(source_hash("fn stable() {}\n"), 0xdc532b0ead0558e9);
    }

    #[tokio::test]
    async fn test_save_index_round_trips_without_leaving_temp_files() {
        let workspace = tempdir().expect("tempdir");
        let config = EmbeddingConfig {
            provider: crate::tools::embed::EmbeddingProvider::Compat,
            model: "text-embedding-test".to_string(),
            url: "https://example.invalid".to_string(),
            api_key: None,
            batch_size: 8,
        };
        let persisted = PersistedSemanticIndex {
            version: SEMANTIC_INDEX_VERSION,
            provider: config.provider.as_str().to_string(),
            model: config.model.clone(),
            chunks: vec![PersistedSemanticChunk {
                chunk_key: "src/lib.rs:1:3:answer".to_string(),
                path: "src/lib.rs".to_string(),
                start_line: 1,
                end_line: 3,
                name: "answer".to_string(),
                content_hash: 42,
                embedding: vec![0.25, 0.75],
            }],
        };
        let updated = PersistedSemanticIndex {
            version: SEMANTIC_INDEX_VERSION,
            provider: config.provider.as_str().to_string(),
            model: config.model.clone(),
            chunks: vec![PersistedSemanticChunk {
                chunk_key: "src/lib.rs:8:12:updated".to_string(),
                path: "src/lib.rs".to_string(),
                start_line: 8,
                end_line: 12,
                name: "updated".to_string(),
                content_hash: 99,
                embedding: vec![0.1, 0.9],
            }],
        };

        save_index(workspace.path(), &persisted)
            .await
            .expect("save index");
        save_index(workspace.path(), &updated)
            .await
            .expect("overwrite index");
        let reloaded = load_index(workspace.path(), &config)
            .await
            .expect("load index");

        assert_eq!(reloaded.chunks.len(), 1);
        assert_eq!(reloaded.chunks[0].chunk_key, updated.chunks[0].chunk_key);
        assert_eq!(
            reloaded.chunks[0].content_hash,
            updated.chunks[0].content_hash
        );
        assert_eq!(reloaded.chunks[0].embedding, updated.chunks[0].embedding);

        let index_dir = workspace.path().join(".vex").join("index");
        let mut entries: Vec<String> = std::fs::read_dir(index_dir)
            .expect("read index dir")
            .map(|entry| {
                entry
                    .expect("dir entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        entries.sort();
        assert_eq!(entries, vec!["semantic-codebase-index.json".to_string()]);
    }

    #[tokio::test]
    async fn test_load_index_resets_for_provider_or_model_changes() {
        let workspace = tempdir().expect("tempdir");
        let compat_config = EmbeddingConfig {
            provider: crate::tools::embed::EmbeddingProvider::Compat,
            model: "model-a".to_string(),
            url: "https://example.invalid".to_string(),
            api_key: None,
            batch_size: 8,
        };
        let native_config = EmbeddingConfig {
            provider: crate::tools::embed::EmbeddingProvider::Native,
            model: "model-b".to_string(),
            url: "http://localhost:9200".to_string(),
            api_key: None,
            batch_size: 1,
        };
        let persisted = PersistedSemanticIndex {
            version: SEMANTIC_INDEX_VERSION,
            provider: compat_config.provider.as_str().to_string(),
            model: compat_config.model.clone(),
            chunks: vec![PersistedSemanticChunk {
                chunk_key: "src/lib.rs:1:3:answer".to_string(),
                path: "src/lib.rs".to_string(),
                start_line: 1,
                end_line: 3,
                name: "answer".to_string(),
                content_hash: 42,
                embedding: vec![0.25, 0.75],
            }],
        };

        save_index(workspace.path(), &persisted)
            .await
            .expect("save index");
        let reloaded = load_index(workspace.path(), &native_config)
            .await
            .expect("load mismatched index");

        assert!(reloaded.chunks.is_empty());
        assert_eq!(reloaded.provider, native_config.provider.as_str());
        assert_eq!(reloaded.model, native_config.model);
    }
}
