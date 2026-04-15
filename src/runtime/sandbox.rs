use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::runtime::CommandRequest;

/// Default macOS sandbox profile. Denies most operations by default but
/// allows broad file access because sandbox-exec'd commands need to read
/// and write project files. Network, IPC, and sysctl-read are required for
/// common development tools (cargo build, git fetch, npm install).
/// Operators who need tighter containment should supply a custom profile
/// via `sandbox_profile`.
const DEFAULT_MACOS_PROFILE: &str = "(version 1)\n\
    (deny default)\n\
    (allow process*)\n\
    (allow file-read*)\n\
    (allow file-write*)\n\
    (allow network*)\n\
    (allow sysctl-read)\n\
    (allow mach-lookup)\n\
    (allow signal)\n";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxKind {
    #[default]
    Passthrough,
    MacosExec,
    Container,
    /// Linux userspace sandbox via bubblewrap (`bwrap`).
    ///
    /// Requires the `bwrap` binary on `PATH`. Falls back to passthrough on
    /// non-Linux platforms or when `bwrap` is not installed (unless
    /// `sandbox_require = true`).
    Bubblewrap,
}

impl SandboxKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passthrough => "passthrough",
            Self::MacosExec => "macos-exec",
            Self::Container => "container",
            Self::Bubblewrap => "bubblewrap",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxConfig {
    #[serde(default)]
    pub kind: SandboxKind,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub require: bool,
}

pub trait SandboxDriver: Send + Sync {
    fn wrap(&self, req: CommandRequest) -> Result<CommandRequest>;

    fn probe(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PassthroughSandbox;

impl SandboxDriver for PassthroughSandbox {
    fn wrap(&self, req: CommandRequest) -> Result<CommandRequest> {
        Ok(req)
    }
}

#[derive(Debug, Clone)]
pub struct MacosSandboxExec {
    profile: Option<String>,
}

impl MacosSandboxExec {
    pub fn new(profile: Option<String>) -> Self {
        Self { profile }
    }
}

impl SandboxDriver for MacosSandboxExec {
    fn wrap(&self, req: CommandRequest) -> Result<CommandRequest> {
        let mut args = Vec::new();
        if let Some(profile) = self
            .profile
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            args.push("-f".to_string());
            args.push(profile.clone());
        } else {
            args.push("-p".to_string());
            args.push(DEFAULT_MACOS_PROFILE.to_string());
        }
        args.push(req.program);
        args.extend(req.args);
        Ok(CommandRequest {
            program: "sandbox-exec".to_string(),
            args,
            working_dir: req.working_dir,
        })
    }

    fn probe(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            if let Some(profile) = self
                .profile
                .as_ref()
                .filter(|value| !value.trim().is_empty())
            {
                let path = PathBuf::from(profile);
                if !path.is_file() {
                    bail!("sandbox profile does not exist: {}", path.display());
                }
            }
            let status = std::process::Command::new("sandbox-exec")
                .arg("-p")
                .arg("(version 1) (allow default)")
                .arg("/usr/bin/true")
                .status()
                .context("failed to execute sandbox-exec probe")?;
            if status.success() {
                Ok(())
            } else {
                bail!("sandbox-exec probe exited with status {status}")
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(anyhow::anyhow!("sandbox-exec is only available on macOS"))
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContainerSandbox {
    image: String,
}

impl ContainerSandbox {
    pub fn new(image: String) -> Result<Self> {
        let image = image.trim().to_string();
        if image.is_empty() {
            bail!("container sandbox requires sandbox_profile to name the image");
        }
        Ok(Self { image })
    }

    fn probe_command(&self) -> Result<std::process::Command> {
        if self.image.trim().is_empty() {
            bail!("container sandbox requires sandbox_profile to name the image");
        }
        let mut command = std::process::Command::new("docker");
        command.args(["run", "--rm", &self.image, "true"]);
        Ok(command)
    }
}

impl SandboxDriver for ContainerSandbox {
    fn wrap(&self, req: CommandRequest) -> Result<CommandRequest> {
        if self.image.trim().is_empty() {
            bail!("container sandbox requires sandbox_profile to name the image");
        }
        let host_dir = req
            .working_dir
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let mount = format!("{}:/workspace", host_dir.display());
        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "-i".to_string(),
            "-w".to_string(),
            "/workspace".to_string(),
            "-v".to_string(),
            mount,
            self.image.clone(),
            req.program,
        ];
        args.extend(req.args);
        Ok(CommandRequest {
            program: "docker".to_string(),
            args,
            working_dir: None,
        })
    }

    fn probe(&self) -> Result<()> {
        let status = self
            .probe_command()?
            .status()
            .context("failed to execute container probe")?;
        if status.success() {
            Ok(())
        } else {
            bail!("container probe exited with status {status}")
        }
    }
}

/// Linux userspace sandbox using `bwrap` (bubblewrap).
///
/// Wraps the target command so that it runs inside a `bwrap` invocation with
/// a minimal bind-mount layout. The working directory is bind-mounted
/// read-write; `/usr`, `/lib`, `/lib64`, `/bin`, `/sbin`, `/etc` are
/// bind-mounted read-only; `/proc`, `/dev`, and `/tmp` are mounted as
/// pseudo-filesystems so that standard toolchains can function. Common host
/// toolchain roots derived from `CARGO_HOME`, `RUSTUP_HOME`, and absolute
/// `PATH` entries are also bind-mounted read-only so installed toolchains stay
/// available inside the sandbox.
///
/// `profile` is an optional whitespace-separated list of additional `bwrap`
/// arguments. The string is split with `split_whitespace()` (no shell quoting
/// or escaping) and injected after the fixed bind mounts and before the `--`
/// separator. Operators can use this to add extra mounts, set env vars
/// (`--setenv`), or opt back in to network access (`--share-net`).
#[derive(Debug, Clone, Default)]
pub struct BubblewrapSandbox {
    /// Extra bwrap arguments parsed from the whitespace-split `profile` string.
    extra_args: Vec<String>,
}

impl BubblewrapSandbox {
    const SYSTEM_READ_ONLY_ROOTS: [&'static str; 6] =
        ["/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc"];

    pub fn new(profile: Option<String>) -> Self {
        let extra_args = profile
            .unwrap_or_default()
            .split_whitespace()
            .map(String::from)
            .collect();
        Self { extra_args }
    }

    fn push_ro_bind_try(args: &mut Vec<String>, path: &Path) {
        let path = path.to_string_lossy().into_owned();
        args.push("--ro-bind-try".to_string());
        args.push(path.clone());
        args.push(path);
    }

    fn extra_read_only_bind_roots(working_dir: &Path) -> Vec<PathBuf> {
        fn maybe_add(
            extra_roots: &mut Vec<PathBuf>,
            working_dir: &Path,
            candidate: Option<PathBuf>,
        ) {
            let Some(path) = candidate else {
                return;
            };
            if !path.is_absolute() || !path.is_dir() {
                return;
            }
            if path == working_dir || path.starts_with(working_dir) {
                return;
            }
            if BubblewrapSandbox::SYSTEM_READ_ONLY_ROOTS
                .iter()
                .map(Path::new)
                .any(|root| path == root || path.starts_with(root))
            {
                return;
            }
            if extra_roots
                .iter()
                .any(|existing| path == *existing || path.starts_with(existing))
            {
                return;
            }
            extra_roots.push(path);
        }

        let mut extra_roots = Vec::new();
        let home_dir = dirs::home_dir();

        maybe_add(
            &mut extra_roots,
            working_dir,
            std::env::var_os("CARGO_HOME")
                .map(PathBuf::from)
                .or_else(|| home_dir.as_ref().map(|home| home.join(".cargo"))),
        );
        maybe_add(
            &mut extra_roots,
            working_dir,
            std::env::var_os("RUSTUP_HOME")
                .map(PathBuf::from)
                .or_else(|| home_dir.as_ref().map(|home| home.join(".rustup"))),
        );
        if let Some(path_var) = std::env::var_os("PATH") {
            for path_entry in std::env::split_paths(&path_var) {
                maybe_add(&mut extra_roots, working_dir, Some(path_entry));
            }
        }

        extra_roots
    }

    /// Build the fixed bind-mount argument list that establishes a usable
    /// root filesystem inside the sandbox.
    fn fixed_bwrap_args(working_dir: &std::path::Path) -> Vec<String> {
        let wd = working_dir.to_string_lossy().into_owned();
        let mut args = vec![
            // Bind the working directory read-write so the command can mutate it.
            "--bind".to_string(),
            wd.clone(),
            wd,
            // Read-only system directories required by most toolchains.
            // Set working directory inside the sandbox.
            "--chdir".to_string(),
            working_dir.to_string_lossy().into_owned(),
        ];
        for root in Self::SYSTEM_READ_ONLY_ROOTS.iter().map(Path::new) {
            Self::push_ro_bind_try(&mut args, root);
        }
        for root in Self::extra_read_only_bind_roots(working_dir) {
            Self::push_ro_bind_try(&mut args, &root);
        }
        args.extend([
            // Pseudo-filesystems that tools expect to exist.
            "--proc".to_string(),
            "/proc".to_string(),
            "--dev".to_string(),
            "/dev".to_string(),
            "--tmpfs".to_string(),
            "/tmp".to_string(),
        ]);
        // Unshare network by default for tighter containment.  Operators who
        // need network access should pass `--share-net` via `profile`.
        args.push("--unshare-net".to_string());
        args
    }
}

impl SandboxDriver for BubblewrapSandbox {
    fn wrap(&self, req: CommandRequest) -> Result<CommandRequest> {
        let working_dir = req
            .working_dir
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let mut args = Self::fixed_bwrap_args(&working_dir);
        args.extend(self.extra_args.clone());
        args.push("--".to_string());
        args.push(req.program);
        args.extend(req.args);

        Ok(CommandRequest {
            program: "bwrap".to_string(),
            args,
            working_dir: None,
        })
    }

    fn probe(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            let status = std::process::Command::new("bwrap")
                .args([
                    "--ro-bind",
                    "/usr",
                    "/usr",
                    "--proc",
                    "/proc",
                    "--dev",
                    "/dev",
                    "--tmpfs",
                    "/tmp",
                    "--unshare-net",
                    "--",
                    "true",
                ])
                .status()
                .context("failed to execute bwrap probe")?;
            if status.success() {
                Ok(())
            } else {
                bail!("bwrap probe exited with status {status}")
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(anyhow::anyhow!(
                "bubblewrap (bwrap) is only available on Linux"
            ))
        }
    }
}

#[derive(Debug, Clone)]
pub enum ConfiguredSandbox {
    Passthrough(PassthroughSandbox),
    MacosExec(MacosSandboxExec),
    Container(ContainerSandbox),
    Bubblewrap(BubblewrapSandbox),
}

impl Default for ConfiguredSandbox {
    fn default() -> Self {
        Self::Passthrough(PassthroughSandbox)
    }
}

impl ConfiguredSandbox {
    pub fn kind(&self) -> SandboxKind {
        match self {
            Self::Passthrough(_) => SandboxKind::Passthrough,
            Self::MacosExec(_) => SandboxKind::MacosExec,
            Self::Container(_) => SandboxKind::Container,
            Self::Bubblewrap(_) => SandboxKind::Bubblewrap,
        }
    }
}

impl SandboxDriver for ConfiguredSandbox {
    fn wrap(&self, req: CommandRequest) -> Result<CommandRequest> {
        match self {
            Self::Passthrough(driver) => driver.wrap(req),
            Self::MacosExec(driver) => driver.wrap(req),
            Self::Container(driver) => driver.wrap(req),
            Self::Bubblewrap(driver) => driver.wrap(req),
        }
    }

    fn probe(&self) -> Result<()> {
        match self {
            Self::Passthrough(driver) => driver.probe(),
            Self::MacosExec(driver) => driver.probe(),
            Self::Container(driver) => driver.probe(),
            Self::Bubblewrap(driver) => driver.probe(),
        }
    }
}

pub fn resolve_configured_sandbox(
    config: &SandboxConfig,
) -> Result<(ConfiguredSandbox, Option<String>)> {
    if config.require && config.kind == SandboxKind::Passthrough {
        bail!("sandbox_require=true requires a non-passthrough sandbox driver");
    }

    let preferred = match config.kind {
        SandboxKind::Passthrough => ConfiguredSandbox::Passthrough(PassthroughSandbox),
        SandboxKind::MacosExec => {
            ConfiguredSandbox::MacosExec(MacosSandboxExec::new(config.profile.clone()))
        }
        SandboxKind::Container => ConfiguredSandbox::Container(ContainerSandbox::new(
            config.profile.clone().unwrap_or_default(),
        )?),
        SandboxKind::Bubblewrap => {
            ConfiguredSandbox::Bubblewrap(BubblewrapSandbox::new(config.profile.clone()))
        }
    };

    match preferred.probe() {
        Ok(()) => Ok((preferred, None)),
        Err(error) if config.require => Err(error.context(format!(
            "sandbox '{}' is required and no fallback is allowed",
            config.kind.as_str()
        ))),
        Err(error) => Ok((
            ConfiguredSandbox::Passthrough(PassthroughSandbox),
            Some(format!(
                "[sandbox] {} unavailable{}: {error}; falling back to passthrough",
                config.kind.as_str(),
                match config.kind {
                    SandboxKind::MacosExec => {
                        " (sandbox-exec is deprecated on modern macOS releases)"
                    }
                    SandboxKind::Bubblewrap => " (bwrap not found or not usable on this platform)",
                    _ => "",
                }
            )),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConfiguredSandbox, PassthroughSandbox, SandboxConfig, SandboxDriver, SandboxKind,
        resolve_configured_sandbox,
    };
    use crate::runtime::CommandRequest;
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    fn assert_ro_bind_present(args: &[String], path: &Path) {
        let needle = path.to_string_lossy().into_owned();
        assert!(
            args.windows(3).any(|window| window[0] == "--ro-bind-try"
                && window[1] == needle
                && window[2] == needle),
            "missing ro-bind for {needle}: {args:?}"
        );
    }

    #[test]
    fn passthrough_sandbox_is_identity() {
        let req = CommandRequest {
            program: "echo".into(),
            args: vec!["hello".into()],
            working_dir: None,
        };
        let wrapped = PassthroughSandbox.wrap(req).expect("wrap request");
        assert_eq!(wrapped.program, "echo");
        assert_eq!(wrapped.args, vec!["hello"]);
    }

    #[test]
    fn sandbox_require_rejects_passthrough() {
        let error = resolve_configured_sandbox(&SandboxConfig {
            kind: SandboxKind::Passthrough,
            profile: None,
            require: true,
        })
        .unwrap_err();
        assert!(error.to_string().contains("sandbox_require=true"));
    }

    #[test]
    fn container_wraps_command_in_container_invocation() {
        let sandbox = ConfiguredSandbox::Container(
            super::ContainerSandbox::new("alpine:3".to_string()).expect("container sandbox"),
        );
        let wrapped = sandbox
            .wrap(CommandRequest {
                program: "echo".into(),
                args: vec!["hello".into()],
                working_dir: Some(PathBuf::from("/tmp")),
            })
            .expect("wrap request");
        assert_eq!(wrapped.program, "docker");
        assert!(wrapped.args.iter().any(|arg| arg == "alpine:3"));
        assert!(wrapped.args.iter().any(|arg| arg == "echo"));
    }

    #[test]
    fn macos_sandbox_exec_wraps_with_default_profile() {
        // When no profile path is provided, wrap must use `-p <inline-profile>`.
        let sandbox = super::MacosSandboxExec::new(None);
        let wrapped = sandbox
            .wrap(CommandRequest {
                program: "cat".into(),
                args: vec!["/etc/hosts".into()],
                working_dir: None,
            })
            .expect("wrap request");
        assert_eq!(wrapped.program, "sandbox-exec");
        let args = &wrapped.args;
        // First option must be -p (inline profile), not -f (profile file).
        assert_eq!(
            args[0], "-p",
            "expected -p option for inline profile, got: {args:?}"
        );
        // The original command and its argument must follow the profile value.
        assert!(
            args.contains(&"cat".to_string()),
            "original program missing: {args:?}"
        );
        assert!(
            args.contains(&"/etc/hosts".to_string()),
            "original arg missing: {args:?}"
        );
    }

    #[test]
    fn macos_sandbox_exec_wraps_with_explicit_profile_file() {
        // When a non-empty profile path is supplied, wrap must use `-f <path>`.
        let profile_path = "/etc/vex-sandbox.sb";
        let sandbox = super::MacosSandboxExec::new(Some(profile_path.to_string()));
        let wrapped = sandbox
            .wrap(CommandRequest {
                program: "ls".into(),
                args: vec!["-la".into()],
                working_dir: None,
            })
            .expect("wrap request");
        assert_eq!(wrapped.program, "sandbox-exec");
        let args = &wrapped.args;
        assert_eq!(
            args[0], "-f",
            "expected -f option for profile file, got: {args:?}"
        );
        assert_eq!(args[1], profile_path, "expected profile path as second arg");
        assert!(
            args.contains(&"ls".to_string()),
            "original program missing: {args:?}"
        );
    }

    #[test]
    fn macos_sandbox_exec_treats_whitespace_only_profile_as_default() {
        // A profile string that is only whitespace must use the default inline
        // profile (same behaviour as None).
        let sandbox = super::MacosSandboxExec::new(Some("   ".to_string()));
        let wrapped = sandbox
            .wrap(CommandRequest {
                program: "true".into(),
                args: vec![],
                working_dir: None,
            })
            .expect("wrap request");
        let args = &wrapped.args;
        assert_eq!(
            args[0], "-p",
            "whitespace-only profile must fall back to -p, got: {args:?}"
        );
    }

    #[test]
    fn container_probe_validates_selected_image_with_run_true() {
        let sandbox =
            super::ContainerSandbox::new("alpine:3".to_string()).expect("container sandbox");
        let command = sandbox.probe_command().expect("build probe command");
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(command.get_program().to_string_lossy(), "docker");
        assert_eq!(args, vec!["run", "--rm", "alpine:3", "true"]);
    }

    #[test]
    fn container_constructor_rejects_empty_image() {
        let error = super::ContainerSandbox::new("   ".to_string()).unwrap_err();
        assert!(error.to_string().contains("sandbox_profile"));
    }

    #[test]
    fn bubblewrap_wraps_command_with_bwrap() {
        let sandbox = super::BubblewrapSandbox::new(None);
        let wrapped = sandbox
            .wrap(CommandRequest {
                program: "echo".into(),
                args: vec!["hello".into()],
                working_dir: Some(PathBuf::from("/tmp")),
            })
            .expect("wrap request");
        assert_eq!(wrapped.program, "bwrap");
        assert!(
            wrapped.args.iter().any(|a| a == "echo"),
            "original program missing: {:?}",
            wrapped.args
        );
        assert!(
            wrapped.args.iter().any(|a| a == "hello"),
            "original arg missing: {:?}",
            wrapped.args
        );
        // working_dir is expressed via --chdir inside the bwrap args, not as a
        // field on CommandRequest.
        assert!(wrapped.working_dir.is_none());
    }

    #[test]
    fn bubblewrap_places_separator_before_command() {
        // `--` must separate bwrap options from the sandboxed command.
        let sandbox = super::BubblewrapSandbox::new(None);
        let wrapped = sandbox
            .wrap(CommandRequest {
                program: "ls".into(),
                args: vec![],
                working_dir: Some(PathBuf::from("/tmp")),
            })
            .expect("wrap request");
        let sep_pos = wrapped
            .args
            .iter()
            .position(|a| a == "--")
            .expect("-- separator missing");
        let cmd_pos = wrapped
            .args
            .iter()
            .position(|a| a == "ls")
            .expect("command missing");
        assert!(
            cmd_pos > sep_pos,
            "command must come after --, sep={sep_pos}, cmd={cmd_pos}"
        );
    }

    #[test]
    fn bubblewrap_extra_profile_args_are_included() {
        // profile args are injected before `--` and the sandboxed command.
        let sandbox = super::BubblewrapSandbox::new(Some("--share-net --setenv FOO bar".into()));
        let wrapped = sandbox
            .wrap(CommandRequest {
                program: "env".into(),
                args: vec![],
                working_dir: Some(PathBuf::from("/tmp")),
            })
            .expect("wrap request");
        let sep_pos = wrapped
            .args
            .iter()
            .position(|a| a == "--")
            .expect("-- separator missing");
        let share_net_pos = wrapped
            .args
            .iter()
            .position(|a| a == "--share-net")
            .expect("--share-net missing");
        assert!(
            share_net_pos < sep_pos,
            "--share-net must come before --, share_net={share_net_pos}, sep={sep_pos}"
        );
    }

    #[test]
    fn bubblewrap_mounts_common_toolchain_roots_and_path_entries() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();
        let _path = crate::test_support::EnvRestore::capture(&_lock, "PATH");
        let _cargo_home = crate::test_support::EnvRestore::capture(&_lock, "CARGO_HOME");
        let _rustup_home = crate::test_support::EnvRestore::capture(&_lock, "RUSTUP_HOME");

        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let cargo_home = temp.path().join("cargo-home");
        let cargo_bin = cargo_home.join("bin");
        let rustup_home = temp.path().join("rustup-home");
        let custom_bin = temp.path().join("custom-bin");

        fs::create_dir_all(&workspace).expect("workspace");
        fs::create_dir_all(&cargo_bin).expect("cargo bin");
        fs::create_dir_all(&rustup_home).expect("rustup home");
        fs::create_dir_all(&custom_bin).expect("custom bin");

        crate::test_support::test_set_var(&_lock, "CARGO_HOME", &cargo_home);
        crate::test_support::test_set_var(&_lock, "RUSTUP_HOME", &rustup_home);
        crate::test_support::test_set_var(
            &_lock,
            "PATH",
            std::env::join_paths([cargo_bin.as_path(), custom_bin.as_path()]).expect("PATH"),
        );

        let wrapped = super::BubblewrapSandbox::new(None)
            .wrap(CommandRequest {
                program: "cargo".into(),
                args: vec!["test".into()],
                working_dir: Some(workspace),
            })
            .expect("wrap request");

        assert_ro_bind_present(&wrapped.args, &cargo_home);
        assert_ro_bind_present(&wrapped.args, &rustup_home);
        assert_ro_bind_present(&wrapped.args, &custom_bin);
    }

    #[test]
    fn bubblewrap_kind_returns_bubblewrap() {
        let sandbox = ConfiguredSandbox::Bubblewrap(super::BubblewrapSandbox::new(None));
        assert_eq!(sandbox.kind(), SandboxKind::Bubblewrap);
    }
}
