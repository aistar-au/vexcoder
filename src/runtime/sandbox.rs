use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
}

impl SandboxKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passthrough => "passthrough",
            Self::MacosExec => "macos-exec",
            Self::Container => "container",
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

#[derive(Debug, Clone)]
pub enum ConfiguredSandbox {
    Passthrough(PassthroughSandbox),
    MacosExec(MacosSandboxExec),
    Container(ContainerSandbox),
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
        }
    }
}

impl SandboxDriver for ConfiguredSandbox {
    fn wrap(&self, req: CommandRequest) -> Result<CommandRequest> {
        match self {
            Self::Passthrough(driver) => driver.wrap(req),
            Self::MacosExec(driver) => driver.wrap(req),
            Self::Container(driver) => driver.wrap(req),
        }
    }

    fn probe(&self) -> Result<()> {
        match self {
            Self::Passthrough(driver) => driver.probe(),
            Self::MacosExec(driver) => driver.probe(),
            Self::Container(driver) => driver.probe(),
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
                if config.kind == SandboxKind::MacosExec {
                    " (sandbox-exec is deprecated on modern macOS releases)"
                } else {
                    ""
                }
            )),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_configured_sandbox, ConfiguredSandbox, PassthroughSandbox, SandboxConfig,
        SandboxDriver, SandboxKind,
    };
    use crate::runtime::CommandRequest;
    use std::path::PathBuf;

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
}
