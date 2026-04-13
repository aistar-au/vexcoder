use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use super::meta_projection::TaskMetadataHeader;

/// Maximum number of metadata entries held in the process-global cache.
/// Sized at 2.5x the default VEX_MAX_STARTUP_TASK_SCANS cap (200) to
/// hold one full scan cycle plus headroom for repeated accesses.
const MAX_METADATA_CACHE_ENTRIES: usize = 512;

/// Fingerprint sufficient to detect that a file has changed since it was
/// last cached. Uses filesystem mtime + size, matching context_cache.rs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileFingerprint {
    len: u64,
    modified: SystemTime,
}

impl FileFingerprint {
    fn from_path(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        let modified = meta.modified().ok()?;
        Some(Self {
            len: meta.len(),
            modified,
        })
    }
}

#[derive(Debug)]
struct CacheEntry {
    header: TaskMetadataHeader,
    fingerprint: FileFingerprint,
    last_access: u64,
    /// Mirrors `TaskMetadataHeader.modified_millis` (JSON `updated_at`) for
    /// tiebreak eviction: equal-tick entries prefer evicting older task files.
    updated_at: u64,
}

#[derive(Debug, Default)]
struct MetadataCache {
    entries: HashMap<PathBuf, CacheEntry>,
    next_access: u64,
}

impl MetadataCache {
    fn tick(&mut self) -> u64 {
        self.next_access = self.next_access.saturating_add(1);
        self.next_access
    }

    fn evict_lru(&mut self) {
        while self.entries.len() > MAX_METADATA_CACHE_ENTRIES {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, e)| (e.last_access, e.updated_at))
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => {
                    self.entries.remove(&k);
                }
                None => break,
            }
        }
    }
}

fn global_metadata_cache() -> &'static Mutex<MetadataCache> {
    static CACHE: OnceLock<Mutex<MetadataCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(MetadataCache::default()))
}

/// Retrieve the metadata header for a task, using the process-global LRU
/// cache. Falls back to `TaskMetadataHeader::from_path` on a cache miss
/// or fingerprint mismatch (file changed since last read).
///
/// Returns `None` if the file cannot be read or parsed; callers should
/// log a tracing::debug and skip the file, matching existing persist.rs
/// error-handling conventions.
pub(crate) fn cached_metadata(path: &Path) -> Option<TaskMetadataHeader> {
    let fingerprint = FileFingerprint::from_path(path)?;
    let cache_key = path.to_path_buf();

    // --- cache probe ---
    {
        let mut cache = global_metadata_cache()
            .lock()
            .expect("metadata cache mutex poisoned");

        // Clone the header while holding a shared borrow, then drop the entry
        // reference before calling `tick()` (which takes &mut self).
        let cached_header = cache.entries.get(&cache_key).and_then(|e| {
            if e.fingerprint == fingerprint {
                Some(e.header.clone())
            } else {
                None
            }
        });

        if let Some(header) = cached_header {
            // cache hit: update LRU access tick
            let tick = cache.tick();
            if let Some(entry) = cache.entries.get_mut(&cache_key) {
                entry.last_access = tick;
            }
            return Some(header);
        }

        // fingerprint mismatch or entry absent: evict if present
        cache.entries.remove(&cache_key);
    }

    // --- cache miss: parse from disk ---
    let header = TaskMetadataHeader::from_path(path).ok()?;

    {
        let mut cache = global_metadata_cache()
            .lock()
            .expect("metadata cache mutex poisoned");
        let tick = cache.tick();
        cache.entries.insert(
            cache_key,
            CacheEntry {
                updated_at: header.modified_millis,
                header: header.clone(),
                fingerprint,
                last_access: tick,
            },
        );
        cache.evict_lru();
    }

    Some(header)
}

#[cfg(test)]
pub(crate) fn reset_metadata_cache_for_tests() {
    *global_metadata_cache()
        .lock()
        .expect("metadata cache mutex poisoned") = MetadataCache::default();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_header_file(dir: &TempDir, id: &str, millis: u64) -> std::path::PathBuf {
        let path = dir.path().join(format!("{id}.json"));
        std::fs::write(
            &path,
            format!(
                r#"{{"id":"{id}","status":"Ready","updated_at":{millis},"active_grants":{{}},"changed_files":[],"command_history":[],"conversation_snapshot":{{"message_count":0,"summary":""}},"interrupted_sessions":[]}}"#
            ),
        )
        .unwrap();
        path
    }

    #[test]
    fn cache_hit_after_first_read() {
        reset_metadata_cache_for_tests();
        let dir = TempDir::new().unwrap();
        let path = write_header_file(&dir, "t1", 100);

        let first = cached_metadata(&path);
        let second = cached_metadata(&path);
        assert!(first.is_some());
        assert!(second.is_some());
        assert_eq!(first.unwrap().id, second.unwrap().id);
    }

    #[test]
    fn cache_invalidates_when_file_changes() {
        reset_metadata_cache_for_tests();
        let dir = TempDir::new().unwrap();
        let path = write_header_file(&dir, "t2", 100);

        cached_metadata(&path);

        // Overwrite with different content and bump mtime
        std::fs::write(&path, r#"{"id":"t2","status":"Completed","updated_at":999,"active_grants":{},"changed_files":[],"command_history":[],"conversation_snapshot":{"message_count":0,"summary":""},"interrupted_sessions":[]}"#).unwrap();
        // Touch mtime explicitly via filetime so the fingerprint changes
        // even on fast file systems.
        filetime::set_file_mtime(&path, filetime::FileTime::from_unix_time(9_999_999, 0)).unwrap();

        let after = cached_metadata(&path);
        assert_eq!(after.unwrap().modified_millis, 999);
    }

    #[test]
    fn cache_evicts_lru_entry_at_cap() {
        reset_metadata_cache_for_tests();
        let dir = TempDir::new().unwrap();

        // Fill cache to MAX+1
        for i in 0..=MAX_METADATA_CACHE_ENTRIES {
            let id = format!("evict-{i}");
            let path = write_header_file(&dir, &id, i as u64);
            cached_metadata(&path);
        }

        {
            let cache = global_metadata_cache().lock().unwrap();
            assert!(
                cache.entries.len() <= MAX_METADATA_CACHE_ENTRIES,
                "cache must not exceed cap; len = {}",
                cache.entries.len()
            );
        }
    }

    #[test]
    fn cache_keys_by_path_when_duplicate_ids_share_a_fingerprint() {
        reset_metadata_cache_for_tests();
        let dir_a = TempDir::new().unwrap();
        let dir_b = TempDir::new().unwrap();
        let path_a = write_header_file(&dir_a, "shared", 100);
        let path_b = write_header_file(&dir_b, "shared", 200);
        let shared_mtime = filetime::FileTime::from_unix_time(1_700_000_000, 0);
        filetime::set_file_mtime(&path_a, shared_mtime).unwrap();
        filetime::set_file_mtime(&path_b, shared_mtime).unwrap();

        let first = cached_metadata(&path_a).unwrap();
        let second = cached_metadata(&path_b).unwrap();

        assert_eq!(first.modified_millis, 100);
        assert_eq!(second.modified_millis, 200);
    }
}
