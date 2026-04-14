//! OS-native credential store access (ADR-024 Gap 38).
//!
//! Wraps the `keyring` crate to provide a uniform read/write/delete surface
//! over macOS Keychain, Linux Secret Service (D-Bus), and Windows Credential
//! Manager.  All operations are fallible: callers must handle errors and fall
//! back to environment variables when the OS credential store is unavailable
//! (headless CI, Docker containers without a D-Bus session).
//!
//! # Service and account identifiers
//!
//! Every keyring entry is keyed by a `(service, account)` pair.  This module
//! reserves the service name `"vexcoder"` for all credentials managed here.
//! The current account names are:
//!
//! | Account        | Description                          |
//! |:---------------|:-------------------------------------|
//! | `"model-token"` | API bearer token (`VEX_MODEL_TOKEN`) |
//!
//! # Environment override
//!
//! Set `VEX_KEYRING_DISABLED=1` (or any non-empty value) to skip all keyring
//! reads and writes.  Useful for CI environments where D-Bus is unavailable
//! or where storing credentials in the keyring is not desired.

use anyhow::{bail, Context, Result};

/// Service name used for all vexcoder keyring entries.
pub const SERVICE: &str = "vexcoder";

/// Account identifier for the model API bearer token.
pub const ACCOUNT_MODEL_TOKEN: &str = "model-token";

/// Returns `true` when the keyring is disabled via `VEX_KEYRING_DISABLED`.
pub fn is_disabled() -> bool {
    std::env::var("VEX_KEYRING_DISABLED")
        .ok()
        .is_some_and(|v| !v.trim().is_empty())
}

/// Read a secret from the OS credential store.
///
/// Returns `Ok(None)` when:
/// - The keyring is disabled via `VEX_KEYRING_DISABLED`.
/// - No entry exists for `(SERVICE, account)`.
/// - The stored value is empty or whitespace-only.
/// - The OS credential store is unavailable (no D-Bus session, etc.).
///
/// Returns `Err` only for unexpected entry access errors that are not
/// "entry not found" (e.g., corrupted keychain data).
pub fn read(account: &str) -> Result<Option<String>> {
    if is_disabled() {
        return Ok(None);
    }
    let entry =
        keyring::Entry::new(SERVICE, account).context("failed to create keyring entry handle")?;
    match entry.get_password() {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value)),
        Ok(_) => Ok(None), // empty stored value treated as absent
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

/// Write a secret to the OS credential store.
///
/// Returns `Err` when the keyring is disabled or the write fails.
pub fn write(account: &str, secret: &str) -> Result<()> {
    if is_disabled() {
        bail!("keyring is disabled via VEX_KEYRING_DISABLED; cannot store credential");
    }
    let entry =
        keyring::Entry::new(SERVICE, account).context("failed to create keyring entry handle")?;
    entry.set_password(secret).context(format!(
        "failed to write keyring entry for account '{account}'"
    ))
}

/// Delete a secret from the OS credential store.
///
/// Returns `Ok(())` when the entry does not exist (idempotent delete).
/// Returns `Err` when the keyring is disabled or an unexpected error occurs.
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

/// Run the `vex credentials` subcommand.
///
/// `sub` is the parsed `CredentialsCommands` variant from the CLI.
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
            println!("vexcoder service: \"{SERVICE}\"");
            println!("  known accounts:");
            println!("    {ACCOUNT_MODEL_TOKEN} — model API bearer token");
        }
    }
    Ok(())
}

/// Parsed credential subcommand action, mirroring `CredentialsCommands` from
/// `cli.rs` but without the clap dependency so the logic lives in the library.
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
        let _guard = crate::test_support::EnvRestore::capture("VEX_KEYRING_DISABLED");

        std::env::remove_var("VEX_KEYRING_DISABLED");
        assert!(!is_disabled(), "should not be disabled when var is unset");

        std::env::set_var("VEX_KEYRING_DISABLED", "1");
        assert!(is_disabled(), "should be disabled when var is '1'");

        std::env::set_var("VEX_KEYRING_DISABLED", "true");
        assert!(is_disabled(), "should be disabled when var is 'true'");

        std::env::set_var("VEX_KEYRING_DISABLED", "");
        assert!(
            !is_disabled(),
            "should not be disabled when var is empty string"
        );
    }

    #[test]
    fn read_returns_none_when_disabled() {
        let _guard = crate::test_support::EnvRestore::capture("VEX_KEYRING_DISABLED");
        std::env::set_var("VEX_KEYRING_DISABLED", "1");
        assert!(read(ACCOUNT_MODEL_TOKEN).unwrap().is_none());
    }

    #[test]
    fn write_errors_when_disabled() {
        let _guard = crate::test_support::EnvRestore::capture("VEX_KEYRING_DISABLED");
        std::env::set_var("VEX_KEYRING_DISABLED", "1");
        assert!(write(ACCOUNT_MODEL_TOKEN, "secret").is_err());
    }

    #[test]
    fn delete_errors_when_disabled() {
        let _guard = crate::test_support::EnvRestore::capture("VEX_KEYRING_DISABLED");
        std::env::set_var("VEX_KEYRING_DISABLED", "1");
        assert!(delete(ACCOUNT_MODEL_TOKEN).is_err());
    }
}
