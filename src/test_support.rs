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
        EnvLockGuard { guard }
    }

    pub async fn lock(&self) -> EnvLockGuard<'_> {
        let guard = self.inner.lock().await;
        EnvLockGuard { guard }
    }
}

impl Default for EnvLock {
    fn default() -> Self {
        Self::new()
    }
}

pub struct EnvLockGuard<'a> {
    guard: AsyncMutexGuard<'a, ()>,
}

impl EnvLockGuard<'_> {
    pub fn set_var(&self, key: &str, value: impl AsRef<std::ffi::OsStr>) {
        let _ = &self.guard;
        // SAFETY: the guard proves exclusive ownership of ENV_LOCK.
        unsafe { std::env::set_var(key, value) }
    }

    pub fn remove_var(&self, key: &str) {
        let _ = &self.guard;
        // SAFETY: the guard proves exclusive ownership of ENV_LOCK.
        unsafe { std::env::remove_var(key) }
    }
}

/// Process-wide lock for tests that mutate environment variables.
/// Use `.blocking_lock()` in sync tests and `.lock().await` in async tests.
pub static ENV_LOCK: EnvLock = EnvLock::new();

/// RAII guard that captures the current value of an environment variable on
/// construction and restores it (or removes it) on drop.  Use this in any
/// test that mutates an env var to ensure neighbouring tests are not affected.
pub struct EnvRestore<'a> {
    _guard: &'a EnvLockGuard<'a>,
    key: &'static str,
    value: Option<String>,
}

impl<'a> EnvRestore<'a> {
    pub fn capture(guard: &'a EnvLockGuard<'a>, key: &'static str) -> Self {
        Self {
            _guard: guard,
            key,
            value: std::env::var(key).ok(),
        }
    }
}

impl Drop for EnvRestore<'_> {
    fn drop(&mut self) {
        match &self.value {
            // SAFETY: EnvRestore cannot outlive the EnvLockGuard it was created from.
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

/// Set an environment variable in test code.
///
/// # Safety contract
/// Callers must pass the `EnvLockGuard` they currently hold.
pub fn test_set_var(lock: &EnvLockGuard<'_>, key: &str, value: impl AsRef<std::ffi::OsStr>) {
    lock.set_var(key, value)
}

/// Remove an environment variable in test code.
///
/// # Safety contract
/// Callers must pass the `EnvLockGuard` they currently hold.
pub fn test_remove_var(lock: &EnvLockGuard<'_>, key: &str) {
    lock.remove_var(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_helpers_allow_locked_mutation() {
        let env_lock = ENV_LOCK.blocking_lock();
        let _guard = EnvRestore::capture(&env_lock, "VEX_TEST_SUPPORT_LOCK_REQUIRED");

        test_set_var(&env_lock, "VEX_TEST_SUPPORT_LOCK_REQUIRED", "1");
        assert_eq!(
            std::env::var("VEX_TEST_SUPPORT_LOCK_REQUIRED").as_deref(),
            Ok("1")
        );

        test_remove_var(&env_lock, "VEX_TEST_SUPPORT_LOCK_REQUIRED");
        assert!(std::env::var("VEX_TEST_SUPPORT_LOCK_REQUIRED").is_err());
    }
}
