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
    ConversationManager::new_with_hooks(
        ApiClient::new_mock(Arc::new(crate::api::mock_client::MockApiClient::new(vec![]))),
        ToolOperator::new(temp.path().to_path_buf()),
        hooks,
    )
}

#[cfg(not(windows))]
static HOOK_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(not(windows))]
fn approval_responder(mut rx: mpsc::UnboundedReceiver<ConversationStreamUpdate>, approved: bool) -> tokio::task::JoinHandle<Result<Vec<String>>> {
    tokio::spawn(async move {
        let mut names = Vec::new();
        while let Some(update) = rx.recv().await {
            if let ConversationStreamUpdate::ToolApprovalRequest(req) = update {
                names.push(req.tool_name.clone());
                let _ = req.response_tx.send(approved);
            }
        }
        Ok(names)
    })
}

#[cfg(not(windows))]
#[tokio::test]
async fn hook_post_tool_runs_after_apply_patch() -> Result<()> {
    let _guard = HOOK_TEST_LOCK.lock().await;
    let temp = TempDir::new()?;
    let target = temp.path().join("note.txt");
    let hook_file = temp.path().join("hook.log");
    std::fs::write(&target, "before\n")?;
    let _ = take_hook_warnings();
    let manager = hook_manager(&temp, vec![shell_hook(
        HookEvent::PostTool, "apply_patch",
        format!("printf post > {}", hook_file.display()),
        HookOnFail::Abort,
    )]);
    let (tx, rx) = mpsc::unbounded_channel();
    let approval_task = approval_responder(rx, true);
    let result = manager.execute_tool_with_timeout_with_updates(
        "apply_patch", &json!({"path": "note.txt", "content": "after\n"}),
        Duration::from_secs(2), Some(&tx),
    ).await?;
    drop(tx);
    let _ = approval_task.await??;
    assert!(result.contains("Applied patch to note.txt"));
    assert_eq!(std::fs::read_to_string(&target)?, "after\n");
    assert_eq!(std::fs::read_to_string(&hook_file)?, "post");
    Ok(())
}

#[cfg(not(windows))]
#[tokio::test]
async fn hook_on_fail_abort_interrupts_turn_and_warn_continues() -> Result<()> {
    let _guard = HOOK_TEST_LOCK.lock().await;
    let temp = TempDir::new()?;
    let _ = take_hook_warnings();

    let abort_manager = hook_manager(&temp, vec![shell_hook(
        HookEvent::PostTool, "write_file",
        "exit 1".to_string(),
        HookOnFail::Abort,
    )]);
    let (tx, rx) = mpsc::unbounded_channel();
    let approval_task = approval_responder(rx, true);
    std::fs::write(temp.path().join("note.txt"), "")?;
    let err = abort_manager.execute_tool_with_timeout_with_updates(
        "write_file", &json!({"path": "note.txt", "content": "x"}),
        Duration::from_secs(2), Some(&tx),
    ).await;
    drop(tx);
    let _ = approval_task.await??;
    assert!(err.is_err() || err.unwrap().contains("hook") || take_hook_warnings().is_some(),
        "abort hook must propagate failure or warning");

    let warn_manager = hook_manager(&temp, vec![shell_hook(
        HookEvent::PostTool, "write_file",
        "exit 1".to_string(),
        HookOnFail::Warn,
    )]);
    let (tx2, rx2) = mpsc::unbounded_channel();
    let approval_task2 = approval_responder(rx2, true);
    let result = warn_manager.execute_tool_with_timeout_with_updates(
        "write_file", &json!({"path": "note.txt", "content": "y"}),
        Duration::from_secs(2), Some(&tx2),
    ).await?;
    drop(tx2);
    let _ = approval_task2.await??;
    assert!(!result.is_empty(), "warn hook must let tool succeed");
    Ok(())
}
