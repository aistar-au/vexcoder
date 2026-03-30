use std::sync::OnceLock;

use anyhow::Result;

use super::Config;

static CONFIG_CACHE: OnceLock<Config> = OnceLock::new();

/// Load the config, caching the result for subsequent calls.
///
/// The first invocation runs the full five-layer resolution chain
/// (environment > repo-local > user > system > defaults). All later
/// calls return a clone of the cached value with zero disk I/O.
pub fn load_cached() -> Result<Config> {
    if let Some(cfg) = CONFIG_CACHE.get() {
        return Ok(cfg.clone());
    }
    let cfg = super::load::load()?;
    Ok(CONFIG_CACHE.get_or_init(|| cfg).clone())
}


#[cfg(test)]
mod tests {
    use crate::config::Config;

    #[test]
    fn load_cached_returns_same_config_twice() {
        // Both calls should succeed and return identical config.
        // The second call hits the cache — no disk I/O.
        let first = Config::load_cached();
        let second = Config::load_cached();
        match (&first, &second) {
            (Ok(a), Ok(b)) => {
                assert_eq!(a.model_name, b.model_name);
                assert_eq!(a.model_url, b.model_url);
            }
            (Err(_), Err(_)) => {} // both fail is ok in env without config
            _ => panic!(
                "load_cached inconsistency: first={:?}, second={:?}",
                first.is_ok(),
                second.is_ok()
            ),
        }
    }
}
