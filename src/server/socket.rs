use anyhow::{bail, Context, Result};
use axum::Router;
use std::path::Path;
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;

use super::ResolvedUnixSurface;

#[cfg(unix)]
pub async fn run_unix_surface(
    router: Router,
    surface: ResolvedUnixSurface,
    shutdown: CancellationToken,
) -> Result<()> {
    let listener = bind_unix_listener(&surface.socket_path)?;
    let cleanup_path = surface.socket_path.clone();
    let result = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            shutdown.cancelled().await;
        })
        .await
        .context("LocalApiServer unix socket exited with an error");
    remove_unix_socket(&cleanup_path)?;
    result
}

#[cfg(unix)]
fn bind_unix_listener(path: &Path) -> Result<tokio::net::UnixListener> {
    if path.exists() {
        let metadata = std::fs::symlink_metadata(path)
            .with_context(|| format!("failed to inspect unix socket '{}'", path.display()))?;
        if metadata.file_type().is_socket() {
            std::fs::remove_file(path).with_context(|| {
                format!("failed to remove stale unix socket '{}'", path.display())
            })?;
        } else {
            bail!(
                "refusing to replace non-socket path '{}'; configure api.socket to a unix socket path",
                path.display()
            );
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create unix socket parent directory '{}'",
                parent.display()
            )
        })?;
    }

    let listener = tokio::net::UnixListener::bind(path)
        .with_context(|| format!("failed to bind unix socket '{}'", path.display()))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).with_context(|| {
        format!(
            "failed to apply 0600 permissions to unix socket '{}'",
            path.display()
        )
    })?;
    Ok(listener)
}

#[cfg(unix)]
fn remove_unix_socket(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path)
            .with_context(|| format!("failed to remove unix socket '{}'", path.display()))?;
    }
    Ok(())
}
