use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use crate::tools::ToolOperator;

const MAX_CACHE_ENTRIES: usize = 64;
const MAX_CACHE_TOTAL_BYTES: usize = 2 * 1024 * 1024;
const MAX_CACHE_FILE_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct CachedFileRead {
    pub(crate) content: String,
    pub(crate) cache_hit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileFingerprint {
    len: u64,
    modified: SystemTime,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    content: String,
    fingerprint: FileFingerprint,
    last_access: u64,
}

#[derive(Debug, Default)]
struct ContextCache {
    entries: HashMap<PathBuf, CacheEntry>,
    total_bytes: usize,
    next_access: u64,
}

impl ContextCache {
    fn next_access_tick(&mut self) -> u64 {
        self.next_access = self.next_access.saturating_add(1);
        self.next_access
    }

    fn evict_to_fit(&mut self) {
        while self.entries.len() > MAX_CACHE_ENTRIES || self.total_bytes > MAX_CACHE_TOTAL_BYTES {
            let Some(oldest_path) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_access)
                .map(|(path, _)| path.clone())
            else {
                break;
            };

            if let Some(removed) = self.entries.remove(&oldest_path) {
                self.total_bytes = self.total_bytes.saturating_sub(removed.content.len());
            }
        }
    }
}

fn global_context_cache() -> &'static Mutex<ContextCache> {
    static CONTEXT_CACHE: OnceLock<Mutex<ContextCache>> = OnceLock::new();
    CONTEXT_CACHE.get_or_init(|| Mutex::new(ContextCache::default()))
}

pub(crate) fn read_cached_file(operator: &ToolOperator, path: &str) -> Result<CachedFileRead> {
    let Some(resolved) = operator.existing_path(path)? else {
        return Ok(CachedFileRead {
            content: operator.read_file(path)?,
            cache_hit: false,
        });
    };

    if resolved.is_dir() {
        return Ok(CachedFileRead {
            content: operator.read_file(path)?,
            cache_hit: false,
        });
    }

    let Ok(fingerprint) = file_fingerprint(&resolved) else {
        return Ok(CachedFileRead {
            content: operator.read_file(path)?,
            cache_hit: false,
        });
    };

    if fingerprint.len > MAX_CACHE_FILE_BYTES {
        return Ok(CachedFileRead {
            content: operator.read_file(path)?,
            cache_hit: false,
        });
    }

    if let Some(content) = try_read_from_cache(&resolved, fingerprint) {
        return Ok(CachedFileRead {
            content,
            cache_hit: true,
        });
    }

    let content = operator.read_file(path)?;
    insert_into_cache(resolved, fingerprint, &content);

    Ok(CachedFileRead {
        content,
        cache_hit: false,
    })
}

fn file_fingerprint(path: &Path) -> Result<FileFingerprint> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("Failed to read metadata for {}", path.display()))?;
    let modified = metadata
        .modified()
        .with_context(|| format!("Failed to read modified time for {}", path.display()))?;

    Ok(FileFingerprint {
        len: metadata.len(),
        modified,
    })
}

fn try_read_from_cache(path: &Path, fingerprint: FileFingerprint) -> Option<String> {
    let mut cache = global_context_cache()
        .lock()
        .expect("context cache mutex poisoned");

    let is_hit = matches!(
        cache.entries.get(path),
        Some(entry) if entry.fingerprint == fingerprint
    );

    if !is_hit {
        if let Some(removed) = cache.entries.remove(path) {
            cache.total_bytes = cache.total_bytes.saturating_sub(removed.content.len());
        }
        return None;
    }

    let access_tick = cache.next_access_tick();
    let entry = cache.entries.get_mut(path)?;
    entry.last_access = access_tick;
    Some(entry.content.clone())
}

fn insert_into_cache(path: PathBuf, fingerprint: FileFingerprint, content: &str) {
    let mut cache = global_context_cache()
        .lock()
        .expect("context cache mutex poisoned");

    if let Some(existing) = cache.entries.remove(&path) {
        cache.total_bytes = cache.total_bytes.saturating_sub(existing.content.len());
    }

    let access_tick = cache.next_access_tick();
    let owned = content.to_string();
    cache.total_bytes = cache.total_bytes.saturating_add(owned.len());
    cache.entries.insert(
        path,
        CacheEntry {
            content: owned,
            fingerprint,
            last_access: access_tick,
        },
    );
    cache.evict_to_fit();
}

#[cfg(test)]
pub(crate) fn reset_context_cache_for_tests() {
    *global_context_cache()
        .lock()
        .expect("context cache mutex poisoned") = ContextCache::default();
}

#[cfg(test)]
mod tests {
    use super::{MAX_CACHE_ENTRIES, read_cached_file, reset_context_cache_for_tests};
    use crate::tools::ToolOperator;
    use filetime::{FileTime, set_file_mtime};
    use std::fs;

    #[test]
    fn test_read_cached_file_hits_after_first_read() {
        reset_context_cache_for_tests();
        let workspace = tempfile::tempdir().expect("tempdir");
        fs::write(workspace.path().join("note.txt"), "alpha\n").expect("write file");
        let operator = ToolOperator::new(workspace.path().to_path_buf());

        let first = read_cached_file(&operator, "note.txt").expect("first read");
        let second = read_cached_file(&operator, "note.txt").expect("second read");

        assert!(!first.cache_hit, "first read must miss the cache");
        assert!(second.cache_hit, "second read must hit the cache");
        assert_eq!(second.content, "alpha\n");
    }

    #[test]
    fn test_read_cached_file_invalidates_when_file_changes() {
        reset_context_cache_for_tests();
        let workspace = tempfile::tempdir().expect("tempdir");
        let path = workspace.path().join("note.txt");
        fs::write(&path, "alpha\n").expect("write original file");
        set_file_mtime(&path, FileTime::from_unix_time(1, 0)).expect("set initial mtime");
        let operator = ToolOperator::new(workspace.path().to_path_buf());

        let first = read_cached_file(&operator, "note.txt").expect("first read");
        let second = read_cached_file(&operator, "note.txt").expect("second read");
        fs::write(&path, "bravo\n").expect("write updated file");
        set_file_mtime(&path, FileTime::from_unix_time(2, 0)).expect("set updated mtime");
        let third = read_cached_file(&operator, "note.txt").expect("third read");

        assert!(!first.cache_hit);
        assert!(second.cache_hit);
        assert!(
            !third.cache_hit,
            "changed file must invalidate the cache entry"
        );
        assert_eq!(third.content, "bravo\n");
    }

    #[test]
    fn test_read_cached_file_evicts_least_recently_used_entry() {
        reset_context_cache_for_tests();
        let workspace = tempfile::tempdir().expect("tempdir");
        let operator = ToolOperator::new(workspace.path().to_path_buf());

        for index in 0..=MAX_CACHE_ENTRIES {
            let name = format!("file-{index}.txt");
            fs::write(workspace.path().join(&name), format!("content {index}\n"))
                .expect("write cache fixture");
            let read = read_cached_file(&operator, &name).expect("populate cache entry");
            assert!(
                !read.cache_hit,
                "first read for {name} must miss before population"
            );
        }

        let reread = read_cached_file(&operator, "file-0.txt").expect("reread oldest entry");
        assert!(
            !reread.cache_hit,
            "oldest entry should be evicted once the cache exceeds its entry cap"
        );
    }
}
