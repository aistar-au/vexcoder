use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{Mutex as AsyncMutex, MutexGuard as AsyncMutexGuard};

pub struct EnvLock {
    inner: AsyncMutex<()>,
}

impl EnvLock {
    pub const fn new() -> Self {
        Self {
            inner: AsyncMutex::const_new(()),
        }
    }

    pub fn blocking_lock(&self) -> EnvLockGuard<'_> {
        let guard = self.inner.blocking_lock();
        ENV_LOCK_HELD.fetch_add(1, Ordering::SeqCst);
        EnvLockGuard { _guard: guard }
    }

    pub async fn lock(&self) -> EnvLockGuard<'_> {
        let guard = self.inner.lock().await;
        ENV_LOCK_HELD.fetch_add(1, Ordering::SeqCst);
        EnvLockGuard { _guard: guard }
    }

    fn assert_held(&self) {
        assert!(
            ENV_LOCK_HELD.load(Ordering::SeqCst) > 0,
            "ENV_LOCK must be held before mutating test environment variables"
        );
    }
}

impl Default for EnvLock {
    fn default() -> Self {
        Self::new()
    }
}

pub struct EnvLockGuard<'a> {
    _guard: AsyncMutexGuard<'a, ()>,
}

impl Drop for EnvLockGuard<'_> {
    fn drop(&mut self) {
        ENV_LOCK_HELD.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Process-wide lock for tests that mutate environment variables.
/// Use `.blocking_lock()` in sync tests and `.lock().await` in async tests.
pub static ENV_LOCK: EnvLock = EnvLock::new();
static ENV_LOCK_HELD: AtomicUsize = AtomicUsize::new(0);

/// RAII guard that captures the current value of an environment variable on
/// construction and restores it (or removes it) on drop.  Use this in any
/// test that mutates an env var to ensure neighbouring tests are not affected.
pub struct EnvRestore {
    key: &'static str,
    value: Option<String>,
}

impl EnvRestore {
    pub fn capture(key: &'static str) -> Self {
        ENV_LOCK.assert_held();
        Self {
            key,
            value: std::env::var(key).ok(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            ENV_LOCK.assert_held();
        }
        match &self.value {
            // SAFETY: all call sites hold ENV_LOCK, preventing concurrent env access.
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

/// Set an environment variable in test code.
///
/// # Safety contract
/// All callers must hold `ENV_LOCK` to prevent data races.
pub fn test_set_var(key: &str, value: impl AsRef<std::ffi::OsStr>) {
    ENV_LOCK.assert_held();
    // SAFETY: callers serialize via ENV_LOCK.
    unsafe { std::env::set_var(key, value) }
}

/// Remove an environment variable in test code.
///
/// # Safety contract
/// All callers must hold `ENV_LOCK` to prevent data races.
pub fn test_remove_var(key: &str) {
    ENV_LOCK.assert_held();
    // SAFETY: callers serialize via ENV_LOCK.
    unsafe { std::env::remove_var(key) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_helpers_allow_locked_mutation() {
        let _lock = ENV_LOCK.blocking_lock();
        let _guard = EnvRestore::capture("VEX_TEST_SUPPORT_LOCK_REQUIRED");

        test_set_var("VEX_TEST_SUPPORT_LOCK_REQUIRED", "1");
        assert_eq!(
            std::env::var("VEX_TEST_SUPPORT_LOCK_REQUIRED").as_deref(),
            Ok("1")
        );

        test_remove_var("VEX_TEST_SUPPORT_LOCK_REQUIRED");
        assert!(std::env::var("VEX_TEST_SUPPORT_LOCK_REQUIRED").is_err());
    }
}
