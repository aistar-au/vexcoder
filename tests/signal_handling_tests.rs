use anyhow::Result;
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};
use tokio_util::sync::CancellationToken;
use vexapi::api::ApiClient;
use vexapi::config::Config;
use vexapi::runtime::context::RuntimeContext;
use vexapi::runtime::{CommandRequest, CommandRunner, DefaultCommandRunner};
use vexapi::state::ConversationManager;
use vexapi::tools::ToolOperator;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_command_session_cancel_signal_completes_promptly() -> Result<()> {
    let runner = DefaultCommandRunner::new();
    let (tx, _rx) = mpsc::channel(16);
    let mut handle = runner.run_streaming(long_running_request(), tx).await?;

    let result = timeout(Duration::from_secs(2), async {
        handle.cancel()?;
        handle.wait().await
    })
    .await
    .expect("command cancellation must complete within timeout")?;

    assert_ne!(
        result.exit_code, 0,
        "cancelled command should not report a clean exit"
    );

    Ok(())
}

#[test]
fn test_runtime_context_cancel_turn_replaces_child_token() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let config = Config::default_for_tui();
    let client = ApiClient::new(&config).expect("api client");
    let manager = ConversationManager::new(client, ToolOperator::new(workspace.path().into()));
    let (update_tx, _update_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut ctx = RuntimeContext::new(manager, update_tx, CancellationToken::new());

    let first = ctx.turn_cancellation_token();
    assert!(!first.is_cancelled(), "fresh turn token must start active");

    ctx.cancel_turn();

    assert!(
        first.is_cancelled(),
        "previous turn token must observe cancellation"
    );

    let second = ctx.turn_cancellation_token();
    assert!(
        !second.is_cancelled(),
        "replacement turn token must be active after cancel_turn"
    );
}
