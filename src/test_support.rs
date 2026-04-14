use tokio::sync::Mutex as AsyncMutex;

/// Process-wide lock for tests that mutate environment variables.
/// Use `.blocking_lock()` in sync tests and `.lock().await` in async tests.
pub static ENV_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

/// RAII guard that captures the current value of an environment variable on
/// construction and restores it (or removes it) on drop.  Use this in any
/// test that mutates an env var to ensure neighbouring tests are not affected.
pub struct EnvRestore {
    key: &'static str,
    value: Option<String>,
}

impl EnvRestore {
    pub fn capture(key: &'static str) -> Self {
        Self {
            key,
            value: std::env::var(key).ok(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        match &self.value {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}
