#[cfg(not(windows))]
use super::*;

#[cfg(not(windows))]
fn shell_hook(event: HookEvent, tool: &str, command: String, on_fail: HookOnFail) -> HookConfig {
    HookConfig {
        event,
        tool: tool.to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), command],
        on_fail,
    }
}
#[cfg(not(windows))]
fn hook_manager(temp: &TempDir, hooks: Vec<HookConfig>) -> ConversationManager {
    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(temp.path().to_path_buf());
    ConversationManager::new_with_hooks(mock_api_client, executor, hooks)
}
#[cfg(not(windows))]
static HOOK_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(not(windows))]
#[derive(Debug)]
struct ApprovalCapture {
    tool_name: String,
    input_preview: String,
}
#[cfg(not(windows))]
fn approval_responder(
    mut rx: mpsc::UnboundedReceiver<ConversationStreamUpdate>,
    approved: bool,
) -> tokio::task::JoinHandle<Result<Vec<ApprovalCapture>>> {
    tokio::spawn(async move {
        let mut requests = Vec::new();
        while let Some(update) = rx.recv().await {
            if let ConversationStreamUpdate::ToolApprovalRequest(request) = update {
                let capture = ApprovalCapture {
                    tool_name: request.tool_name.clone(),
                    input_preview: request.input_preview.clone(),
                };
                let _ = request.response_tx.send(approved);
                requests.push(capture);
            }
        }
        Ok(requests)
    })
}
#[cfg(not(windows))]
#[tokio::test]
async fn test_hook_post_apply_patch_runs_command() -> Result<()> {
    let _hook_guard = HOOK_TEST_LOCK.lock().await;
    let temp = TempDir::new()?;
    let target = temp.path().join("note.txt");
    let hook_file = temp.path().join("hook.log");
    std::fs::write(&target, "before\n")?;
    let _ = take_hook_warnings();
    let manager = hook_manager(
        &temp,
        vec![shell_hook(
            HookEvent::PostTool,
            "apply_patch",
            format!("printf post > {}", hook_file.display()),
            HookOnFail::Abort,
        )],
    );

    let (tx, rx) = mpsc::unbounded_channel();
    let approval_task = approval_responder(rx, true);

    let result = manager
        .execute_tool_with_timeout_with_updates(
            "apply_patch",
            &json!({"path": "note.txt", "content": "after\n"}),
            Duration::from_secs(2),
            Some(&tx),
        )
        .await?;
    drop(tx);
    let requests = approval_task.await??;

    assert!(result.contains("Applied patch to note.txt"));
    assert_eq!(requests.len(), 1);
    assert_eq!(std::fs::read_to_string(&target)?, "after\n");
    assert_eq!(std::fs::read_to_string(&hook_file)?, "post");
    Ok(())
}
#[cfg(not(windows))]
#[tokio::test]
async fn test_hook_pre_tool_runs_before_dispatch() -> Result<()> {
    let _hook_guard = HOOK_TEST_LOCK.lock().await;
    let temp = TempDir::new()?;
    let target = temp.path().join("note.txt");
    let hook_file = temp.path().join("hook.log");
    let _ = take_hook_warnings();
    let manager = hook_manager(
        &temp,
        vec![shell_hook(
            HookEvent::PreTool,
            "write_file",
            format!(
                "[ ! -e {} ] && printf pre > {}",
                target.display(),
                hook_file.display()
            ),
            HookOnFail::Abort,
        )],
    );

    let (tx, rx) = mpsc::unbounded_channel();
    let approval_task = approval_responder(rx, true);
    let result = manager
        .execute_tool_with_timeout_with_updates(
            "write_file",
            &json!({"path": "note.txt", "content": "hello\n"}),
            Duration::from_secs(2),
            Some(&tx),
        )
        .await?;
    drop(tx);
    let requests = approval_task.await??;

    assert!(result.contains("Wrote note.txt"));
    assert_eq!(requests.len(), 1);
    assert_eq!(std::fs::read_to_string(&target)?, "hello\n");
    assert_eq!(std::fs::read_to_string(&hook_file)?, "pre");
    Ok(())
}

#[cfg(not(windows))]
#[tokio::test]
async fn test_http_hook_timeout_aborts_turn() -> Result<()> {
    let _hook_guard = HOOK_TEST_LOCK.lock().await;
    let temp = TempDir::new()?;
    let target = temp.path().join("note.txt");
    let _ = take_hook_warnings();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let accept_task = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.expect("accept hung hook socket");
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let mock_api_client = ApiClient::new_mock(Arc::new(
        crate::api::mock_client::MockApiClient::new(vec![]),
    ));
    let executor = ToolOperator::new(temp.path().to_path_buf());
    let manager = ConversationManager::new_with_hooks_full(
        mock_api_client,
        executor,
        Vec::new(),
        vec![crate::config::HttpHookConfig {
            event: HookEvent::PreTool,
            tool: "write_file".to_string(),
            url: format!("http://{addr}/hang"),
            on_fail: HookOnFail::Abort,
        }],
    );

    let (tx, rx) = mpsc::unbounded_channel();
    let approval_task = approval_responder(rx, true);
    let result = manager
        .execute_tool_with_timeout_with_updates(
            "write_file",
            &json!({"path": "note.txt", "content": "hello\n"}),
            Duration::from_millis(200),
            Some(&tx),
        )
        .await;
    drop(tx);
    approval_task.await??;
    accept_task.abort();

    let err = result.expect_err("timed out HTTP hook must abort the tool turn");
    let message = format!("{err:#}");
    assert!(
        message.contains("timed out after 200ms"),
        "unexpected timeout message: {message}"
    );
    assert!(
        !target.exists(),
        "timed out pre-tool hook must prevent writes"
    );
    Ok(())
}
#[cfg(not(windows))]
#[tokio::test]
async fn test_hook_on_fail_abort_interrupts_turn() -> Result<()> {
    let _hook_guard = HOOK_TEST_LOCK.lock().await;
    let temp = TempDir::new()?;
    let target = temp.path().join("note.txt");
    let _ = take_hook_warnings();
    let manager = hook_manager(
        &temp,
        vec![shell_hook(
            HookEvent::PreTool,
            "write_file",
            "exit 17".to_string(),
            HookOnFail::Abort,
        )],
    );

    let (tx, rx) = mpsc::unbounded_channel();
    let approval_task = approval_responder(rx, true);
    let result = manager
        .execute_tool_with_timeout_with_updates(
            "write_file",
            &json!({"path": "note.txt", "content": "hello\n"}),
            Duration::from_secs(2),
            Some(&tx),
        )
        .await;
    drop(tx);
    approval_task.await??;

    let err = result.expect_err("aborting hook must stop the tool turn");
    let message = format!("{err:#}");
    assert!(
        message.contains("exit code 17"),
        "unexpected error: {message}"
    );
    assert!(!target.exists(), "pre-tool abort must prevent file writes");
    Ok(())
}
#[cfg(not(windows))]
#[tokio::test]
async fn test_hook_on_fail_warn_continues() -> Result<()> {
    let _hook_guard = HOOK_TEST_LOCK.lock().await;
    let temp = TempDir::new()?;
    let target = temp.path().join("note.txt");
    let _ = take_hook_warnings();
    let manager = hook_manager(
        &temp,
        vec![shell_hook(
            HookEvent::PreTool,
            "write_file",
            "exit 9".to_string(),
            HookOnFail::Warn,
        )],
    );

    let (tx, rx) = mpsc::unbounded_channel();
    let approval_task = approval_responder(rx, true);
    let result = manager
        .execute_tool_with_timeout_with_updates(
            "write_file",
            &json!({"path": "note.txt", "content": "hello\n"}),
            Duration::from_secs(2),
            Some(&tx),
        )
        .await;
    drop(tx);
    let requests = approval_task.await??;

    let result = result?;
    let warnings = take_hook_warnings();
    assert_eq!(requests.len(), 1);
    assert!(result.contains("Wrote note.txt"));
    assert_eq!(std::fs::read_to_string(&target)?, "hello\n");
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("[hooks] warning: hook 'sh' failed with exit code 9")),
        "expected warn path in hook warnings, got: {warnings:?}"
    );
    Ok(())
}
#[cfg(not(windows))]
#[tokio::test]
async fn test_hook_requires_run_command_approval() -> Result<()> {
    let _hook_guard = HOOK_TEST_LOCK.lock().await;
    let temp = TempDir::new()?;
    let target = temp.path().join("note.txt");
    let hook_file = temp.path().join("hook.log");
    let _ = take_hook_warnings();
    let manager = hook_manager(
        &temp,
        vec![shell_hook(
            HookEvent::PreTool,
            "write_file",
            format!("printf pre > {}", hook_file.display()),
            HookOnFail::Abort,
        )],
    );

    let (tx, rx) = mpsc::unbounded_channel();
    let approval_task = approval_responder(rx, false);
    let result = manager
        .execute_tool_with_timeout_with_updates(
            "write_file",
            &json!({"path": "note.txt", "content": "hello\n"}),
            Duration::from_secs(2),
            Some(&tx),
        )
        .await;
    drop(tx);
    let requests = approval_task.await??;

    let result = result?;
    let warnings = take_hook_warnings();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].tool_name, "run_command");
    assert!(
        requests[0]
            .input_preview
            .contains("\"tool\": \"write_file\"")
            || requests[0]
                .input_preview
                .contains("\"tool\":\"write_file\""),
        "approval preview should identify the wrapped tool: {}",
        requests[0].input_preview
    );
    assert!(result.contains("Wrote note.txt"));
    assert_eq!(std::fs::read_to_string(&target)?, "hello\n");
    assert!(!hook_file.exists(), "denied hook must be skipped");
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("missing RunCommand approval")),
        "expected approval warning, got: {warnings:?}"
    );
    Ok(())
}
#[cfg(not(windows))]
#[tokio::test]
async fn test_hook_skipped_without_approval_emits_warning() -> Result<()> {
    let _hook_guard = HOOK_TEST_LOCK.lock().await;
    let temp = TempDir::new()?;
    let target = temp.path().join("note.txt");
    let hook_file = temp.path().join("hook.log");
    let _ = take_hook_warnings();
    let manager = hook_manager(
        &temp,
        vec![shell_hook(
            HookEvent::PostTool,
            "write_file",
            format!("printf post > {}", hook_file.display()),
            HookOnFail::Abort,
        )],
    );

    let result = manager
        .execute_tool_with_timeout(
            "write_file",
            &json!({"path": "note.txt", "content": "hello\n"}),
            Duration::from_secs(2),
        )
        .await;
    let result = result?;
    let warnings = take_hook_warnings();
    assert!(result.contains("Wrote note.txt"));
    assert_eq!(std::fs::read_to_string(&target)?, "hello\n");
    assert!(
        !hook_file.exists(),
        "hook must not run when approval context is absent"
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("missing RunCommand approval")),
        "expected missing-approval warning, got: {warnings:?}"
    );
    Ok(())
}
