use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::runtime::ModelBackendKind;
use crate::types::ModelProfile;

/// Walk ancestors of `cwd` to find the nearest `.vex/config.toml`.
/// The resolved `working_dir` from the merged config must not influence
/// which file is selected — always walk from the actual process cwd.
pub(crate) fn find_repo_local_config(cwd: &Path) -> Option<PathBuf> {
    let mut dir: &Path = cwd;
    loop {
        let candidate = dir.join(".vex").join("config.toml");
        if candidate.exists() {
            return Some(candidate);
        }
        dir = dir.parent()?;
    }
}

pub(crate) fn find_repo_root(cwd: &Path) -> Option<PathBuf> {
    let mut dir: &Path = cwd;
    loop {
        if dir.join(".git").exists() || dir.join(".vex").join("config.toml").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

pub(crate) fn user_config_path() -> Option<PathBuf> {
    user_config_xdg_path()
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn preferred_home_dir() -> Option<PathBuf> {
    env_path("HOME").or_else(dirs::home_dir)
}

fn user_config_xdg_path() -> Option<PathBuf> {
    env_path("XDG_CONFIG_HOME")
        .or_else(dirs::config_dir)
        .map(|d| d.join("vex").join("config.toml"))
}

pub(crate) fn system_config_path() -> Option<PathBuf> {
    Some(PathBuf::from("/etc/vex/config.toml"))
}

pub(crate) fn expand_home(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/")
        && let Some(home) = preferred_home_dir()
    {
        return home.join(rest);
    }
    path
}

pub(crate) fn resolve_working_dir(working_dir: Option<PathBuf>, fallback_cwd: &Path) -> PathBuf {
    working_dir.unwrap_or_else(|| fallback_cwd.to_path_buf())
}

pub(crate) fn resolve_profile_base_dir(cwd: &Path, repo_cfg: Option<&Path>) -> PathBuf {
    if let Some(root) = repo_cfg
        .and_then(|config| config.parent())
        .and_then(Path::parent)
    {
        return root.to_path_buf();
    }
    find_repo_root(cwd).unwrap_or_else(|| cwd.to_path_buf())
}

pub(crate) fn load_model_profile(
    selected_path: Option<&Path>,
    profile_base_dir: &Path,
    model_backend: ModelBackendKind,
) -> Result<ModelProfile> {
    let Some(path) = selected_path else {
        return Ok(ModelProfile::default_for_backend(model_backend));
    };

    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        profile_base_dir.join(path)
    };
    ModelProfile::load(&resolved).with_context(|| {
        format!(
            "failed to load model profile '{}' (base '{}')",
            path.display(),
            profile_base_dir.display()
        )
    })
}
