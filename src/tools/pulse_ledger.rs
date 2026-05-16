use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

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

const MAX_PULSE_LEDGER_ENTRIES: usize = 64;

#[derive(Debug, Default)]
struct PulseLedger {
    entries: Vec<LocatedRead>,
}

fn global_pulse_ledger() -> &'static Mutex<PulseLedger> {
    static PULSE_LEDGER: OnceLock<Mutex<PulseLedger>> = OnceLock::new();
    PULSE_LEDGER.get_or_init(|| Mutex::new(PulseLedger::default()))
}

#[cfg(test)]
static PULSE_LEDGER_TEST_LOCK: Mutex<()> = Mutex::new(());

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

#[cfg(test)]
pub(crate) fn lock_pulse_ledger_for_tests() -> std::sync::MutexGuard<'static, ()> {
    PULSE_LEDGER_TEST_LOCK
        .lock()
        .expect("pulse ledger test mutex poisoned")
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_PULSE_LEDGER_ENTRIES, clear_pulse_ledger, file_fingerprint, find_pulse_read,
        lock_pulse_ledger_for_tests, record_pulse_read,
    };
    use filetime::{FileTime, set_file_mtime};
    use std::fs;

    #[test]
    fn test_pulse_ledger_records_and_finds_matching_fingerprint() {
        let _lock = lock_pulse_ledger_for_tests();
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
        let _lock = lock_pulse_ledger_for_tests();
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
    fn test_pulse_ledger_evicts_oldest_when_capped() {
        let _lock = lock_pulse_ledger_for_tests();
        clear_pulse_ledger();
        let workspace = tempfile::tempdir().expect("tempdir");

        let mut paths = Vec::new();
        for index in 0..(MAX_PULSE_LEDGER_ENTRIES + 4) {
            let name = format!("note-{index}.txt");
            let path = workspace.path().join(&name);
            fs::write(&path, format!("line {index}\n")).expect("write fixture");
            let fingerprint = file_fingerprint(&path).expect("fingerprint");
            record_pulse_read(path.clone(), 1, 1, fingerprint);
            paths.push(path);
        }

        for evicted in &paths[..4] {
            assert!(
                find_pulse_read(evicted).is_none(),
                "the oldest entries must be evicted once the ledger exceeds its cap"
            );
        }
        let kept = paths.last().expect("at least one entry recorded");
        assert!(
            find_pulse_read(kept).is_some(),
            "the most recent entry must remain after eviction"
        );
        clear_pulse_ledger();
    }
}
