/// Startup-phase resource budget.
///
/// Controls how many task-state files are scanned during cold-start
/// discovery and how long metadata cache entries remain valid.
///
/// All fields are read from environment variables at construction time.
/// Callers should construct a `StartupBudget::default()` once at the
/// call-site rather than re-constructing per scan iteration.
#[derive(Debug, Clone)]
pub struct StartupBudget {
    /// Maximum number of task-state files to scan at startup.
    ///
    /// Set via `VEX_MAX_STARTUP_TASK_SCANS`. Default: 200.
    /// When the cap is reached, the N most-recently-modified task files
    /// are returned; older files are silently excluded from discovery.
    /// The UI should surface a hint when the cap is active.
    pub max_scans: usize,

    /// TTL for metadata cache entries in milliseconds.
    ///
    /// Set via `VEX_STARTUP_CACHE_TTL_MS`. Default: 300_000 (5 minutes).
    /// Reserved for future use; the current cache implementation uses
    /// fingerprint-based invalidation (mtime + size) rather than TTL.
    pub cache_ttl_ms: u64,

    /// Enable verbose allocation tracing to stderr.
    ///
    /// Set `VEX_TRACE_STARTUP_ALLOC=1` to activate. Default: off.
    /// When active, emits `[startup-alloc]` lines to stderr with file
    /// counts and byte totals for each scan pass.
    pub trace_allocations: bool,
}

impl Default for StartupBudget {
    fn default() -> Self {
        Self {
            max_scans: std::env::var("VEX_MAX_STARTUP_TASK_SCANS")
                .ok()
                .and_then(|s| s.trim().parse::<usize>().ok())
                .filter(|&n| n > 0)
                .unwrap_or(200),
            cache_ttl_ms: std::env::var("VEX_STARTUP_CACHE_TTL_MS")
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(300_000),
            trace_allocations: std::env::var("VEX_TRACE_STARTUP_ALLOC")
                .as_deref()
                .unwrap_or("")
                == "1",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ENV_LOCK;

    #[test]
    fn defaults_to_200_when_env_unset() {
        let _env_lock = ENV_LOCK.blocking_lock();
        std::env::remove_var("VEX_MAX_STARTUP_TASK_SCANS");
        std::env::remove_var("VEX_STARTUP_CACHE_TTL_MS");
        std::env::remove_var("VEX_TRACE_STARTUP_ALLOC");

        let budget = StartupBudget::default();
        assert_eq!(budget.max_scans, 200);
        assert_eq!(budget.cache_ttl_ms, 300_000);
        assert!(!budget.trace_allocations);
    }

    #[test]
    fn rejects_zero_max_scans_and_falls_back_to_default() {
        let _env_lock = ENV_LOCK.blocking_lock();
        std::env::set_var("VEX_MAX_STARTUP_TASK_SCANS", "0");

        let budget = StartupBudget::default();

        std::env::remove_var("VEX_MAX_STARTUP_TASK_SCANS");
        assert_eq!(budget.max_scans, 200);
    }
}
