#[derive(Debug, Clone)]
pub struct StartupBudget {
    pub max_scans: usize,

    pub cache_ttl_ms: u64,

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
        crate::test_support::test_remove_var(&_env_lock, "VEX_MAX_STARTUP_TASK_SCANS");
        crate::test_support::test_remove_var(&_env_lock, "VEX_STARTUP_CACHE_TTL_MS");
        crate::test_support::test_remove_var(&_env_lock, "VEX_TRACE_STARTUP_ALLOC");

        let budget = StartupBudget::default();
        assert_eq!(budget.max_scans, 200);
        assert_eq!(budget.cache_ttl_ms, 300_000);
        assert!(!budget.trace_allocations);
    }

    #[test]
    fn rejects_zero_max_scans_and_falls_back_to_default() {
        let _env_lock = ENV_LOCK.blocking_lock();
        crate::test_support::test_set_var(&_env_lock, "VEX_MAX_STARTUP_TASK_SCANS", "0");

        let budget = StartupBudget::default();

        crate::test_support::test_remove_var(&_env_lock, "VEX_MAX_STARTUP_TASK_SCANS");
        assert_eq!(budget.max_scans, 200);
    }
}
