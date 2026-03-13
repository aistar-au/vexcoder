use crate::terminal::TerminalGuard;
use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
#[cfg(unix)]
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::oneshot;

fn validate_working_dir(working_dir: &Path) -> Result<()> {
    if !working_dir.exists() {
        anyhow::bail!(
            "working directory does not exist: {}",
            working_dir.display()
        );
    }
    if !working_dir.is_dir() {
        anyhow::bail!(
            "working directory is not a directory: {}",
            working_dir.display()
        );
    }
    Ok(())
}

pub struct CommandRequest {
    pub program: String,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
}

pub struct CommandResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub struct OutputChunk {
    pub stream: StreamKind,
    pub text: String,
}

pub enum StreamKind {
    Stdout,
    Stderr,
}

pub struct CommandHandle {
    cancel_tx: Option<oneshot::Sender<()>>,
}

impl CommandHandle {
    /// Returns true if this handle has not yet sent a cancel signal.
    /// Only used in tests to assert handle state after `cancel` is called.
    #[cfg(test)]
    pub fn is_consumed(&self) -> bool {
        self.cancel_tx.is_none()
    }
}

pub struct PtySession {
    reader: Box<dyn Read + Send>,
    _pty_process: Box<dyn portable_pty::Child + Send>,
}

pub trait CommandRunner: Send + Sync {
    fn run_one_shot(
        &self,
        req: CommandRequest,
    ) -> impl std::future::Future<Output = Result<CommandResult>> + Send;
    fn run_streaming(
        &self,
        req: CommandRequest,
        tx: tokio::sync::mpsc::Sender<OutputChunk>,
    ) -> impl std::future::Future<Output = Result<CommandHandle>> + Send;
    fn cancel(&self, handle: CommandHandle)
        -> impl std::future::Future<Output = Result<()>> + Send;
    fn attach_pty(&self, req: CommandRequest) -> Result<PtySession>;
}

pub struct DefaultCommandRunner;

#[cfg(unix)]
struct ParentSigintGuard {
    task: tokio::task::JoinHandle<()>,
}

#[cfg(unix)]
impl ParentSigintGuard {
    fn ignore() -> Result<Self> {
        let mut interrupts = signal(SignalKind::interrupt())
            .context("failed to subscribe to SIGINT while running passthrough command")?;
        let task = tokio::spawn(async move { while interrupts.recv().await.is_some() {} });
        Ok(Self { task })
    }
}

#[cfg(unix)]
impl Drop for ParentSigintGuard {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl DefaultCommandRunner {
    pub fn new() -> Self {
        Self
    }

    pub async fn run_passthrough_one_shot(&self, req: CommandRequest) -> Result<CommandResult> {
        let mut command = Command::new(&req.program);
        command.args(&req.args);
        if let Some(working_dir) = &req.working_dir {
            validate_working_dir(working_dir)?;
            command.current_dir(working_dir);
        }
        command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        let _guard = TerminalGuard::yield_for_command()
            .with_context(|| format!("failed to yield terminal for command: {}", req.program))?;
        #[cfg(unix)]
        let _sigint_guard = ParentSigintGuard::ignore()?;

        let status = command
            .status()
            .await
            .with_context(|| format!("failed to execute command: {}", req.program))?;

        Ok(CommandResult {
            exit_code: status.code().unwrap_or(-1),
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

impl Default for DefaultCommandRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandRunner for DefaultCommandRunner {
    async fn run_one_shot(&self, req: CommandRequest) -> Result<CommandResult> {
        let mut command = Command::new(&req.program);
        command.args(&req.args);
        if let Some(working_dir) = &req.working_dir {
            validate_working_dir(working_dir)?;
            command.current_dir(working_dir);
        }
        command.kill_on_drop(true);
        let output = command
            .output()
            .await
            .with_context(|| format!("Failed to execute command: {}", req.program))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        Ok(CommandResult {
            exit_code,
            stdout,
            stderr,
        })
    }

    async fn run_streaming(
        &self,
        req: CommandRequest,
        tx: tokio::sync::mpsc::Sender<OutputChunk>,
    ) -> Result<CommandHandle> {
        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();

        let mut command = Command::new(&req.program);
        command.args(&req.args);
        if let Some(working_dir) = &req.working_dir {
            validate_working_dir(working_dir)?;
            command.current_dir(working_dir);
        }

        let mut process = command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("Failed to spawn command: {}", req.program))?;

        let stdout = process.stdout.take().context("Failed to capture stdout")?;
        let stderr = process.stderr.take().context("Failed to capture stderr")?;

        let tx_stdout = tx.clone();
        let stdout_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if tx_stdout
                    .send(OutputChunk {
                        stream: StreamKind::Stdout,
                        text: line + "\n",
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        let tx_stderr = tx;
        let stderr_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if tx_stderr
                    .send(OutputChunk {
                        stream: StreamKind::Stderr,
                        text: line + "\n",
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        tokio::spawn(async move {
            tokio::select! {
                _ = cancel_rx => {
                    let _ = process.kill().await;
                }
                _ = stdout_task => {}
                _ = stderr_task => {}
            }
            let _ = process.wait().await;
        });

        Ok(CommandHandle {
            cancel_tx: Some(cancel_tx),
        })
    }

    async fn cancel(&self, handle: CommandHandle) -> Result<()> {
        if let Some(tx) = handle.cancel_tx {
            let _ = tx.send(());
        }
        Ok(())
    }

    fn attach_pty(&self, req: CommandRequest) -> Result<PtySession> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Failed to open PTY")?;

        let mut cmd = CommandBuilder::new(&req.program);
        for arg in &req.args {
            cmd.arg(arg);
        }
        if let Some(working_dir) = &req.working_dir {
            validate_working_dir(working_dir)?;
            cmd.cwd(working_dir);
        }
        let pty_process = pair
            .slave
            .spawn_command(cmd)
            .context("Failed to spawn command in PTY")?;

        let reader = pair
            .master
            .try_clone_reader()
            .context("Failed to clone reader")?;

        Ok(PtySession {
            reader,
            _pty_process: pty_process,
        })
    }
}

impl PtySession {
    pub fn read_output(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo_request() -> CommandRequest {
        if cfg!(windows) {
            CommandRequest {
                program: "cmd".into(),
                args: vec!["/C".into(), "echo".into(), "hello".into()],
                working_dir: None,
            }
        } else {
            CommandRequest {
                program: "echo".into(),
                args: vec!["hello".into()],
                working_dir: None,
            }
        }
    }

    fn long_running_request() -> CommandRequest {
        if cfg!(windows) {
            CommandRequest {
                program: "cmd".into(),
                args: vec![
                    "/C".into(),
                    "ping".into(),
                    "-n".into(),
                    "30".into(),
                    "127.0.0.1".into(),
                ],
                working_dir: None,
            }
        } else {
            CommandRequest {
                program: "sleep".into(),
                args: vec!["30".into()],
                working_dir: None,
            }
        }
    }

    #[tokio::test]
    async fn test_command_runner_one_shot_captures_exit_code_and_stdout() {
        let runner = DefaultCommandRunner::new();
        let req = echo_request();
        let result = runner.run_one_shot(req).await.expect("run failed");
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn test_command_runner_cancel_transitions_to_cancelling() {
        let runner = DefaultCommandRunner::new();

        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let req = long_running_request();

        let handle = runner.run_streaming(req, tx).await.expect("spawn failed");
        assert!(!handle.is_consumed());

        let result =
            tokio::time::timeout(tokio::time::Duration::from_secs(2), runner.cancel(handle)).await;

        assert!(result.is_ok(), "cancel must complete within 2 seconds");
        assert!(result.unwrap().is_ok(), "cancel must not error");
    }

    #[tokio::test]
    async fn test_passthrough_one_shot_reports_exit_code() {
        let runner = DefaultCommandRunner::new();
        let req = echo_request();
        let result = runner
            .run_passthrough_one_shot(req)
            .await
            .expect("passthrough run failed");
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }
}
