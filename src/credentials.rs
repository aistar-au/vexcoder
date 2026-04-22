

use anyhow::{Context, Result, bail};


pub const SERVICE: &str = "vexapi";


pub const ACCOUNT_MODEL_TOKEN: &str = "model-token";


pub fn is_disabled() -> bool {
    std::env::var("VEX_KEYRING_DISABLED")
        .ok()
        .is_some_and(|v| !v.trim().is_empty())
}


pub fn read(account: &str) -> Result<Option<String>> {
    if is_disabled() {
        return Ok(None);
    }
    let entry =
        keyring::Entry::new(SERVICE, account).context("failed to create keyring entry handle")?;
    match entry.get_password() {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value)),
        Ok(_) => Ok(None), 
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(keyring::Error::NoStorageAccess(inner)) => {
            tracing::debug!(
                account,
                error = %inner,
                "keyring storage unavailable; credential absent"
            );
            Ok(None)
        }
        Err(other) => Err(other).context(format!(
            "unexpected error reading keyring entry for account '{account}'"
        )),
    }
}


pub fn write(account: &str, secret: &str) -> Result<()> {
    if is_disabled() {
        bail!("keyring is disabled via VEX_KEYRING_DISABLED; cannot store credential");
    }
    if secret.trim().is_empty() {
        bail!("secret must not be empty or whitespace-only");
    }
    let entry =
        keyring::Entry::new(SERVICE, account).context("failed to create keyring entry handle")?;
    entry.set_password(secret).context(format!(
        "failed to write keyring entry for account '{account}'"
    ))
}


pub fn delete(account: &str) -> Result<()> {
    if is_disabled() {
        bail!("keyring is disabled via VEX_KEYRING_DISABLED; cannot delete credential");
    }
    let entry =
        keyring::Entry::new(SERVICE, account).context("failed to create keyring entry handle")?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(other) => Err(other).context(format!(
            "failed to delete keyring entry for account '{account}'"
        )),
    }
}


pub fn run_credentials(sub: CredentialsAction) -> Result<()> {
    match sub {
        CredentialsAction::Set { account, secret } => {
            write(&account, &secret)?;
            println!("credential '{account}' stored in OS keyring");
        }
        CredentialsAction::Get { account } => match read(&account)? {
            Some(value) => println!("{value}"),
            None => bail!("no credential found for account '{account}'"),
        },
        CredentialsAction::Delete { account } => {
            delete(&account)?;
            println!("credential '{account}' removed (or was already absent)");
        }
        CredentialsAction::List => {
            println!("vexapi service: \"{SERVICE}\"");
            println!("  known accounts:");
            println!("    {ACCOUNT_MODEL_TOKEN} — model API bearer token");
        }
    }
    Ok(())
}


#[derive(Debug)]
pub enum CredentialsAction {
    Set { account: String, secret: String },
    Get { account: String },
    Delete { account: String },
    List,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_disabled_respects_env_var() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        let _guard = crate::test_support::EnvRestore::capture(&_lock, "VEX_KEYRING_DISABLED");

        crate::test_support::test_remove_var(&_lock, "VEX_KEYRING_DISABLED");
        assert!(!is_disabled(), "should not be disabled when var is unset");

        crate::test_support::test_set_var(&_lock, "VEX_KEYRING_DISABLED", "1");
        assert!(is_disabled(), "should be disabled when var is '1'");

        crate::test_support::test_set_var(&_lock, "VEX_KEYRING_DISABLED", "true");
        assert!(is_disabled(), "should be disabled when var is 'true'");

        crate::test_support::test_set_var(&_lock, "VEX_KEYRING_DISABLED", "");
        assert!(
            !is_disabled(),
            "should not be disabled when var is empty string"
        );
    }

    #[test]
    fn read_returns_none_when_disabled() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        let _guard = crate::test_support::EnvRestore::capture(&_lock, "VEX_KEYRING_DISABLED");
        crate::test_support::test_set_var(&_lock, "VEX_KEYRING_DISABLED", "1");
        assert!(read(ACCOUNT_MODEL_TOKEN).unwrap().is_none());
    }

    #[test]
    fn write_errors_when_disabled() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        let _guard = crate::test_support::EnvRestore::capture(&_lock, "VEX_KEYRING_DISABLED");
        crate::test_support::test_set_var(&_lock, "VEX_KEYRING_DISABLED", "1");
        assert!(write(ACCOUNT_MODEL_TOKEN, "secret").is_err());
    }

    #[test]
    fn write_errors_on_empty_secret() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        let _guard = crate::test_support::EnvRestore::capture(&_lock, "VEX_KEYRING_DISABLED");
        crate::test_support::test_remove_var(&_lock, "VEX_KEYRING_DISABLED");
        let err = write(ACCOUNT_MODEL_TOKEN, "   ").unwrap_err();
        assert!(
            err.to_string().contains("empty"),
            "expected empty-secret error, got: {err}"
        );
    }

    #[test]
    fn delete_errors_when_disabled() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        let _guard = crate::test_support::EnvRestore::capture(&_lock, "VEX_KEYRING_DISABLED");
        crate::test_support::test_set_var(&_lock, "VEX_KEYRING_DISABLED", "1");
        assert!(delete(ACCOUNT_MODEL_TOKEN).is_err());
    }
}
