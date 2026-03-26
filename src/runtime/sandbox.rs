use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::runtime::CommandRequest;

const DEFAULT_MACOS_PROFILE: &str =
    "(version 1)\n(deny default)\n(allow process*)\n(allow file-read*)\n(allow file-write*)\n";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxKind {
    #[default]
    Passthrough,
    MacosExec,
    Docker,
}

impl SandboxKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passthrough => "passthrough",
            Self::MacosExec => "macos-exec",
            Self::Docker => "docker",
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
pub struct DockerSandbox {
    image: String,
}

impl DockerSandbox {
    pub fn new(image: String) -> Self {
        Self { image }
    }
}

impl SandboxDriver for DockerSandbox {
    fn wrap(&self, req: CommandRequest) -> Result<CommandRequest> {
        if self.image.trim().is_empty() {
            bail!("docker sandbox requires sandbox_profile to name the image");
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
        if self.image.trim().is_empty() {
            bail!("docker sandbox requires sandbox_profile to name the image");
        }
        let status = std::process::Command::new("docker")
            .args(["info", "--format", "{{.ServerVersion}}"])
            .status()
            .context("failed to execute docker probe")?;
        if status.success() {
            Ok(())
        } else {
            bail!("docker probe exited with status {status}")
        }
    }
}

#[derive(Debug, Clone)]
pub enum ConfiguredSandbox {
    Passthrough(PassthroughSandbox),
    MacosExec(MacosSandboxExec),
    Docker(DockerSandbox),
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
            Self::Docker(_) => SandboxKind::Docker,
        }
    }
}

impl SandboxDriver for ConfiguredSandbox {
    fn wrap(&self, req: CommandRequest) -> Result<CommandRequest> {
        match self {
            Self::Passthrough(driver) => driver.wrap(req),
            Self::MacosExec(driver) => driver.wrap(req),
            Self::Docker(driver) => driver.wrap(req),
        }
    }

    fn probe(&self) -> Result<()> {
        match self {
            Self::Passthrough(driver) => driver.probe(),
            Self::MacosExec(driver) => driver.probe(),
            Self::Docker(driver) => driver.probe(),
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
        SandboxKind::Docker => ConfiguredSandbox::Docker(DockerSandbox::new(
            config.profile.clone().unwrap_or_default(),
        )),
    };

    match preferred.probe() {
        Ok(()) => Ok((preferred, None)),
        Err(error) if config.kind == SandboxKind::Passthrough => Err(error),
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
    fn docker_wraps_command_in_container_invocation() {
        let sandbox = ConfiguredSandbox::Docker(super::DockerSandbox::new("alpine:3".to_string()));
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
}
