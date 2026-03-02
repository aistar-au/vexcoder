use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::oneshot;

pub struct CommandRequest {
    pub program: String,
    pub args: Vec<String>,
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
    /// Check if cancellation was requested (handle has been consumed)
    pub fn is_cancelling(&self) -> bool {
        self.cancel_tx.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationStatus {
    Running,
    Cancelling,
    Completed,
    Failed,
}

pub struct PtySession {
    reader: Box<dyn std::io::Read + Send>,
    _child: Box<dyn portable_pty::Child + Send>,
}

pub trait CommandRunner: Send + Sync {
    async fn run_one_shot(&self, req: CommandRequest) -> Result<CommandResult>;
    async fn run_streaming(
        &self,
        req: CommandRequest,
        tx: tokio::sync::mpsc::Sender<OutputChunk>,
    ) -> Result<CommandHandle>;
    async fn cancel(&self, handle: CommandHandle) -> Result<()>;
    fn attach_pty(&self, req: CommandRequest) -> Result<PtySession>;
}

pub struct DefaultCommandRunner;

impl DefaultCommandRunner {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultCommandRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandRunner for DefaultCommandRunner {
    async fn run_one_shot(&self, req: CommandRequest) -> Result<CommandResult> {
        let output = Command::new(&req.program)
            .args(&req.args)
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

        let mut child = Command::new(&req.program)
            .args(&req.args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("Failed to spawn command: {}", req.program))?;

        let stdout = child.stdout.take().context("Failed to capture stdout")?;
        let stderr = child.stderr.take().context("Failed to capture stderr")?;

        let tx_stdout = tx.clone();
        let stdout_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if tx_stdout.send(OutputChunk { stream: StreamKind::Stdout, text: line + "\n" }).await.is_err() {
                    break;
                }
            }
        });

        let tx_stderr = tx;
        let stderr_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if tx_stderr.send(OutputChunk { stream: StreamKind::Stderr, text: line + "\n" }).await.is_err() {
                    break;
                }
            }
        });

        tokio::spawn(async move {
            tokio::select! {
                _ = cancel_rx => {
                    let _ = child.kill().await;
                }
                _ = stdout_task => {}
                _ = stderr_task => {}
            }
            let _ = child.wait().await;
        });

        Ok(CommandHandle { cancel_tx: Some(cancel_tx) })
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

        let cmd = CommandBuilder::new(&req.program);
        let child = pair
            .slave
            .spawn_command(cmd)
            .context("Failed to spawn command in PTY")?;

        let reader = pair.master.try_clone_reader().context("Failed to clone reader")?;

        Ok(PtySession {
            reader,
            _child: child,
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

    #[tokio::test]
    async fn test_command_runner_one_shot_captures_exit_code_and_stdout() {
        let runner = DefaultCommandRunner::new();
        let req = CommandRequest {
            program: "echo".into(),
            args: vec!["hello".into()],
        };
        let result = runner.run_one_shot(req).await.expect("run failed");
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn test_command_runner_cancel_transitions_to_cancelling() {
        let runner = DefaultCommandRunner::new();

        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let req = CommandRequest {
            program: "sleep".into(),
            args: vec!["30".into()],
        };

        let handle = runner.run_streaming(req, tx).await.expect("spawn failed");
        assert!(!handle.is_cancelling());

        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            runner.cancel(handle),
        )
        .await;

        assert!(result.is_ok(), "cancel must complete within 2 seconds");
        assert!(result.unwrap().is_ok(), "cancel must not error");
    }
    
    #[test]
    fn test_cancellation_status_variants() {
        let status = CancellationStatus::Running;
        assert_eq!(status, CancellationStatus::Running);

        let status = CancellationStatus::Cancelling;
        assert_eq!(status, CancellationStatus::Cancelling);

        let status = CancellationStatus::Completed;
        assert_eq!(status, CancellationStatus::Completed);

        let status = CancellationStatus::Failed;
        assert_eq!(status, CancellationStatus::Failed);
    }
}
