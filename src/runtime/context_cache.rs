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
pub(crate) struct FileFingerprint {
    pub(crate) len: u64,
    pub(crate) modified: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocatedRead {
    pub(crate) path: PathBuf,
    pub(crate) start_line: usize,
    pub(crate) end_line: usize,
    pub(crate) fingerprint: FileFingerprint,
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

#[cfg(test)]
static CONTEXT_CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());

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

pub(crate) fn file_fingerprint(path: &Path) -> Result<FileFingerprint> {
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

const MAX_PULSE_LEDGER_ENTRIES: usize = 64;

#[derive(Debug, Default)]
struct PulseLedger {
    entries: Vec<LocatedRead>,
}

fn global_pulse_ledger() -> &'static Mutex<PulseLedger> {
    static PULSE_LEDGER: OnceLock<Mutex<PulseLedger>> = OnceLock::new();
    PULSE_LEDGER.get_or_init(|| Mutex::new(PulseLedger::default()))
}

pub(crate) fn record_pulse_read(
    path: PathBuf,
    start_line: usize,
    end_line: usize,
    fingerprint: FileFingerprint,
) {
    let mut ledger = global_pulse_ledger()
        .lock()
        .expect("pulse ledger mutex poisoned");
    ledger.entries.retain(|entry| entry.path != path);
    ledger.entries.push(LocatedRead {
        path,
        start_line,
        end_line,
        fingerprint,
    });
    while ledger.entries.len() > MAX_PULSE_LEDGER_ENTRIES {
        ledger.entries.remove(0);
    }
}

pub(crate) fn find_pulse_read(path: &Path) -> Option<LocatedRead> {
    let current = file_fingerprint(path).ok()?;
    let ledger = global_pulse_ledger()
        .lock()
        .expect("pulse ledger mutex poisoned");
    ledger
        .entries
        .iter()
        .rev()
        .find(|entry| entry.path == path && entry.fingerprint == current)
        .cloned()
}

pub(crate) fn clear_pulse_ledger() {
    let mut ledger = global_pulse_ledger()
        .lock()
        .expect("pulse ledger mutex poisoned");
    ledger.entries.clear();
}

pub(crate) fn pulse_ledger_snapshot() -> Vec<LocatedRead> {
    let ledger = global_pulse_ledger()
        .lock()
        .expect("pulse ledger mutex poisoned");
    ledger.entries.clone()
}

pub(crate) fn restore_pulse_ledger(entries: Vec<LocatedRead>) {
    let mut ledger = global_pulse_ledger()
        .lock()
        .expect("pulse ledger mutex poisoned");
    ledger.entries = entries;
    while ledger.entries.len() > MAX_PULSE_LEDGER_ENTRIES {
        ledger.entries.remove(0);
    }
}

pub(crate) struct RangeReadOutcome {
    pub(crate) rendered: String,
    pub(crate) start_line: usize,
    pub(crate) end_line: usize,
    pub(crate) fingerprint: Option<FileFingerprint>,
    pub(crate) resolved_path: Option<PathBuf>,
}

pub(crate) fn read_cached_file_range(
    operator: &ToolOperator,
    path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<RangeReadOutcome> {
    let resolved = operator.existing_path(path)?;
    let (content, fingerprint, resolved_path) = match resolved {
        Some(resolved) if !resolved.is_dir() => match file_fingerprint(&resolved) {
            Ok(fingerprint) if fingerprint.len <= MAX_CACHE_FILE_BYTES => {
                let cached = if let Some(content) = try_read_from_cache(&resolved, fingerprint) {
                    content
                } else {
                    let fresh = operator.read_file(path)?;
                    insert_into_cache(resolved.clone(), fingerprint, &fresh);
                    fresh
                };
                (cached, Some(fingerprint), Some(resolved))
            }
            Ok(fingerprint) => {
                let fresh = operator.read_file(path)?;
                (fresh, Some(fingerprint), Some(resolved))
            }
            Err(_) => {
                let fresh = operator.read_file(path)?;
                (fresh, None, Some(resolved))
            }
        },
        _ => {
            let fresh = operator.read_file(path)?;
            (fresh, None, None)
        }
    };

    let user_offset = offset.unwrap_or(1);
    let start = user_offset.saturating_sub(1);
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if start >= total {
        return Ok(RangeReadOutcome {
            rendered: format!("(file has {total} lines, offset {user_offset} is past end)"),
            start_line: user_offset,
            end_line: user_offset,
            fingerprint,
            resolved_path,
        });
    }
    let end = limit
        .map(|value| (start + value).min(total))
        .unwrap_or(total);
    let selected: Vec<String> = lines[start..end]
        .iter()
        .enumerate()
        .map(|(index, line)| format!("{:>5}\t{}", start + index + 1, line))
        .collect();
    let header = if start > 0 || end < total {
        format!("(showing lines {}-{} of {total})\n", start + 1, end)
    } else {
        String::new()
    };
    Ok(RangeReadOutcome {
        rendered: format!("{}{}", header, selected.join("\n")),
        start_line: start + 1,
        end_line: end,
        fingerprint,
        resolved_path,
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
pub(crate) fn lock_context_cache_for_tests() -> std::sync::MutexGuard<'static, ()> {
    CONTEXT_CACHE_TEST_LOCK
        .lock()
        .expect("context cache test mutex poisoned")
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_CACHE_ENTRIES, MAX_PULSE_LEDGER_ENTRIES, clear_pulse_ledger, file_fingerprint,
        find_pulse_read, lock_context_cache_for_tests, pulse_ledger_snapshot, read_cached_file,
        read_cached_file_range, record_pulse_read, reset_context_cache_for_tests,
        restore_pulse_ledger,
    };
    use crate::tools::ToolOperator;
    use filetime::{FileTime, set_file_mtime};
    use std::fs;

    #[test]
    fn test_read_cached_file_hits_after_first_read() {
        let _lock = lock_context_cache_for_tests();
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
        let _lock = lock_context_cache_for_tests();
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
        let _lock = lock_context_cache_for_tests();
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

    #[test]
    fn test_pulse_ledger_records_and_finds_matching_fingerprint() {
        let _lock = lock_context_cache_for_tests();
        reset_context_cache_for_tests();
        clear_pulse_ledger();
        let workspace = tempfile::tempdir().expect("tempdir");
        let path = workspace.path().join("note.txt");
        fs::write(&path, "alpha\nbravo\ncharlie\n").expect("write file");
        let fingerprint = file_fingerprint(&path).expect("fingerprint");
        record_pulse_read(path.clone(), 1, 3, fingerprint);

        let hit = find_pulse_read(&path).expect("ledger should remember the read");
        assert_eq!(hit.start_line, 1);
        assert_eq!(hit.end_line, 3);
        assert_eq!(hit.fingerprint, fingerprint);
        clear_pulse_ledger();
    }

    #[test]
    fn test_pulse_ledger_misses_after_file_mutates() {
        let _lock = lock_context_cache_for_tests();
        reset_context_cache_for_tests();
        clear_pulse_ledger();
        let workspace = tempfile::tempdir().expect("tempdir");
        let path = workspace.path().join("note.txt");
        fs::write(&path, "alpha\n").expect("write original");
        set_file_mtime(&path, FileTime::from_unix_time(1, 0)).expect("set initial mtime");
        let fingerprint = file_fingerprint(&path).expect("fingerprint");
        record_pulse_read(path.clone(), 1, 1, fingerprint);

        fs::write(&path, "alpha changed\n").expect("mutate file");
        set_file_mtime(&path, FileTime::from_unix_time(2, 0)).expect("bump mtime");

        assert!(
            find_pulse_read(&path).is_none(),
            "ledger must miss when the file fingerprint no longer matches"
        );
        clear_pulse_ledger();
    }

    #[test]
    fn test_pulse_ledger_snapshot_and_restore_roundtrip() {
        let _lock = lock_context_cache_for_tests();
        reset_context_cache_for_tests();
        clear_pulse_ledger();
        let workspace = tempfile::tempdir().expect("tempdir");
        let path = workspace.path().join("note.txt");
        fs::write(&path, "alpha\n").expect("write file");
        let fingerprint = file_fingerprint(&path).expect("fingerprint");
        record_pulse_read(path.clone(), 1, 1, fingerprint);

        let snapshot = pulse_ledger_snapshot();
        clear_pulse_ledger();
        assert!(find_pulse_read(&path).is_none());

        restore_pulse_ledger(snapshot);
        assert!(
            find_pulse_read(&path).is_some(),
            "restoring a ledger snapshot should make the entry queryable again"
        );
        clear_pulse_ledger();
    }

    #[test]
    fn test_pulse_ledger_caps_at_max_entries() {
        let _lock = lock_context_cache_for_tests();
        reset_context_cache_for_tests();
        clear_pulse_ledger();
        let workspace = tempfile::tempdir().expect("tempdir");

        for index in 0..(MAX_PULSE_LEDGER_ENTRIES + 4) {
            let name = format!("note-{index}.txt");
            let path = workspace.path().join(&name);
            fs::write(&path, format!("line {index}\n")).expect("write fixture");
            let fingerprint = file_fingerprint(&path).expect("fingerprint");
            record_pulse_read(path, 1, 1, fingerprint);
        }

        let snapshot = pulse_ledger_snapshot();
        assert!(
            snapshot.len() <= MAX_PULSE_LEDGER_ENTRIES,
            "ledger must cap at MAX_PULSE_LEDGER_ENTRIES, got {}",
            snapshot.len()
        );
        clear_pulse_ledger();
    }

    #[test]
    fn test_read_cached_file_range_reports_range_and_fingerprint() {
        let _lock = lock_context_cache_for_tests();
        reset_context_cache_for_tests();
        clear_pulse_ledger();
        let workspace = tempfile::tempdir().expect("tempdir");
        fs::write(
            workspace.path().join("note.txt"),
            "alpha\nbravo\ncharlie\ndelta\n",
        )
        .expect("write file");
        let operator = ToolOperator::new(workspace.path().to_path_buf());

        let outcome =
            read_cached_file_range(&operator, "note.txt", Some(2), Some(2)).expect("range read");
        assert_eq!(outcome.start_line, 2);
        assert_eq!(outcome.end_line, 3);
        assert!(outcome.rendered.contains("bravo"));
        assert!(outcome.rendered.contains("charlie"));
        assert!(!outcome.rendered.contains("delta"));
        assert!(outcome.fingerprint.is_some());
        assert!(outcome.resolved_path.is_some());
        clear_pulse_ledger();
    }
}
