use super::*;
use crate::api::{ApiClient, mock_client::MockApiClient};
use crate::runtime::UiUpdate;
use crate::state::ConversationManager;
use crate::tools::ToolOperator;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;

fn make_runtime_context() -> (RuntimeContext, mpsc::UnboundedReceiver<UiUpdate>) {
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    let (tx, rx) = mpsc::unbounded_channel::<UiUpdate>();
    (
        RuntimeContext::new(conversation, tx, CancellationToken::new()),
        rx,
    )
}

#[tokio::test]
async fn test_edit_loop_terminates_at_max_turns() {
    let mut edit_loop = EditLoop::new("task-001".to_string()).with_max_turns(1);
    let (mut ctx, _rx) = make_runtime_context();
    let cancel = CancellationToken::new();

    let outcome = edit_loop
        .run(
            "edit src/runtime/edit_loop.rs".to_string(),
            &mut ctx,
            &cancel,
        )
        .await
        .expect("run should succeed");

    assert!(matches!(outcome, EditLoopOutcome::MaxTurnsReached { .. }));
}

#[tokio::test]
async fn test_edit_loop_returns_cancelled_when_token_is_pre_cancelled() {
    let mut edit_loop = EditLoop::new("task-002".to_string());
    let (mut ctx, _rx) = make_runtime_context();
    let cancel = CancellationToken::new();
    cancel.cancel();

    let outcome = edit_loop
        .run(
            "edit src/runtime/edit_loop.rs".to_string(),
            &mut ctx,
            &cancel,
        )
        .await
        .expect("run should succeed");

    assert!(matches!(outcome, EditLoopOutcome::Cancelled));
}

#[test]
fn test_edit_loop_detects_dirty_workspace_for_target_paths() {
    let workspace = tempfile::tempdir().expect("tempdir");
    fs::write(workspace.path().join("tracked.txt"), "v1\n").expect("seed file");
    run_git(workspace.path(), &["init"]);
    run_git(workspace.path(), &["add", "tracked.txt"]);
    run_git(
        workspace.path(),
        &[
            "-c",
            "user.name=vex-test",
            "-c",
            "user.email=vex-test@example.com",
            "commit",
            "-m",
            "init",
        ],
    );

    let clean = EditLoop::check_workspace_dirty(workspace.path(), &[PathBuf::from("tracked.txt")])
        .expect("clean check");
    assert!(!clean, "workspace should be clean after commit");

    fs::write(workspace.path().join("tracked.txt"), "v2\n").expect("mutate file");
    let dirty = EditLoop::check_workspace_dirty(workspace.path(), &[PathBuf::from("tracked.txt")])
        .expect("dirty check");
    assert!(dirty, "workspace should be dirty after tracked file change");
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("git should start");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_edit_loop_emits_dirty_workspace_warning() {
    let workspace = tempfile::tempdir().expect("tempdir");
    fs::write(workspace.path().join("target.rs"), "fn main() {}\n").expect("seed file");
    run_git(workspace.path(), &["init"]);
    run_git(workspace.path(), &["add", "target.rs"]);
    run_git(
        workspace.path(),
        &[
            "-c",
            "user.name=vex-test",
            "-c",
            "user.email=vex-test@example.com",
            "commit",
            "-m",
            "init",
        ],
    );

    let clean = EditLoop::check_workspace_dirty(workspace.path(), &[PathBuf::from("target.rs")])
        .expect("clean check");
    assert!(
        !clean,
        "workspace should report clean immediately after commit"
    );

    fs::write(
        workspace.path().join("target.rs"),
        "fn main() { /* dirty */ }\n",
    )
    .expect("mutate");
    let dirty = EditLoop::check_workspace_dirty(workspace.path(), &[PathBuf::from("target.rs")])
        .expect("dirty check");
    assert!(
        dirty,
        "workspace should report dirty after tracked file change"
    );
}

#[tokio::test]
async fn test_edit_loop_cancel_mid_validation() {
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        tokio::task::yield_now().await;
        cancel_clone.cancel();
    });

    let mut edit_loop = EditLoop::new("task-cancel-mid".to_string()).with_max_turns(4);
    let (mut ctx, _rx) = make_runtime_context();
    let outcome = edit_loop
        .run("edit src/lib.rs".to_string(), &mut ctx, &cancel)
        .await
        .expect("run should not error");

    assert!(
        matches!(outcome, EditLoopOutcome::Cancelled),
        "loop must return Cancelled when token fires mid-run"
    );
}

fn final_text_turn(message_id: &str, text: &str) -> Vec<String> {
    vec![
        format!(
            r#"event: message_start
data: {{"type":"message_start","message":{{"id":"{message_id}","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{{"input_tokens":10,"output_tokens":1}}}}}}"#
        ),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#
            .to_string(),
        format!(
            r#"event: content_block_delta
data: {{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"{text}"}}}}"#
        ),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#
            .to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#
            .to_string(),
    ]
}

fn write_file_turn(message_id: &str, tool_id: &str, path: &str, content: &str) -> Vec<String> {
    let partial_json = serde_json::json!({
        "path": path,
        "content": content,
    })
    .to_string();
    let partial_json = serde_json::to_string(&partial_json).expect("partial json string");

    vec![
        format!(
            r#"event: message_start
data: {{"type":"message_start","message":{{"id":"{message_id}","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{{"input_tokens":10,"output_tokens":1}}}}}}"#
        ),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_write_01","name":"write_file","input":{}}}"#
            .replace("toolu_write_01", tool_id),
        format!(
            r#"event: content_block_delta
data: {{"type":"content_block_delta","index":0,"delta":{{"type":"input_json_delta","partial_json":{partial_json}}}}}"#
        ),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#
            .to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":8}}"#
            .to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#
            .to_string(),
    ]
}

#[tokio::test]
async fn test_edit_loop_skips_validation_when_no_patch_is_applied() {
    let workspace = tempfile::tempdir().expect("tempdir");
    run_git(workspace.path(), &["init"]);

    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![final_text_turn(
        "msg-no-patch",
        "I need more context before editing.",
    )])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    let (tx, _rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());
    let mut edit_loop = EditLoop::new("task-no-patch".to_string())
        .with_max_turns(1)
        .with_working_dir(workspace.path().to_path_buf());
    let cancel = CancellationToken::new();

    let outcome = edit_loop
        .run("edit src/lib.rs".to_string(), &mut ctx, &cancel)
        .await
        .expect("run should succeed");

    assert!(matches!(outcome, EditLoopOutcome::MaxTurnsReached { .. }));
    assert!(
        edit_loop.last_validation_result().is_none(),
        "validation must not run when the pulse applied no patch"
    );
}

#[tokio::test]
async fn test_edit_loop_validation_failure_retries_after_patch_and_stops_at_max_turns() {
    let workspace = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(workspace.path().join(".vex")).expect("create .vex");
    std::fs::create_dir_all(workspace.path().join("src")).expect("create src");
    #[cfg(not(windows))]
    std::fs::write(
        workspace.path().join(".vex/validate.toml"),
        r#"[[commands]]
label = "forced failure"
program = "sh"
args = ["-c", "printf 'still failing\n' >&2; exit 1"]
"#,
    )
    .expect("write validate config");
    #[cfg(windows)]
    std::fs::write(
        workspace.path().join(".vex/validate.toml"),
        r#"[[commands]]
label = "forced failure"
program = "cmd"
args = ["/C", "echo still failing 1>&2 && exit /b 1"]
"#,
    )
    .expect("write validate config");

    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![
        write_file_turn(
            "msg-write-1",
            "toolu_write_1",
            "src/lib.rs",
            "pub const RETRY_ROUND: usize = 1;\n",
        ),
        final_text_turn("msg-final-1", "Applied the first patch."),
        write_file_turn(
            "msg-write-2",
            "toolu_write_2",
            "src/lib.rs",
            "pub const RETRY_ROUND: usize = 2;\n",
        ),
        final_text_turn("msg-final-2", "Applied the retry patch."),
    ])));
    let conversation =
        ConversationManager::new(client, ToolOperator::new(workspace.path().to_path_buf()));
    let (tx, mut rx) = mpsc::unbounded_channel::<UiUpdate>();
    let mut ctx = RuntimeContext::new(conversation, tx, CancellationToken::new());
    let transcript = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let transcript_reader = Arc::clone(&transcript);

    let reader = tokio::spawn(async move {
        while let Some(update) = rx.recv().await {
            match update {
                UiUpdate::ToolApprovalRequest(request) => {
                    let _ = request.response_tx.send(true);
                }
                UiUpdate::TranscriptLine(line) => {
                    transcript_reader.lock().unwrap().push(line);
                }
                UiUpdate::StreamDelta(_)
                | UiUpdate::ServerMetadata(_)
                | UiUpdate::StreamBlockStart { .. }
                | UiUpdate::StreamBlockDelta { .. }
                | UiUpdate::ToolCallArgumentsUpdated { .. }
                | UiUpdate::StreamBlockComplete { .. }
                | UiUpdate::CommandSessionStarted { .. }
                | UiUpdate::CommandSessionAttached { .. }
                | UiUpdate::CommandSessionFinished { .. }
                | UiUpdate::PulseComplete
                | UiUpdate::EditLoopComplete { .. }
                | UiUpdate::Error(_)
                | UiUpdate::ContextCompacted { .. } => {}
            }
        }
    });

    let mut edit_loop = EditLoop::new("task-validation-retry".to_string())
        .with_max_turns(2)
        .with_working_dir(workspace.path().to_path_buf());
    let cancel = CancellationToken::new();

    let outcome = edit_loop
        .run("edit src/lib.rs".to_string(), &mut ctx, &cancel)
        .await
        .expect("run should succeed");

    drop(ctx);
    reader.await.expect("reader task");

    assert!(matches!(outcome, EditLoopOutcome::MaxTurnsReached { .. }));
    let transcript = transcript.lock().unwrap();
    assert!(
        transcript
            .iter()
            .any(|line| line.contains("[edit loop: validation failed, retrying]")),
        "validation failure must trigger retry guidance: {:?}",
        *transcript
    );
    assert!(
        transcript
            .iter()
            .any(|line| line.contains("[edit loop pulse 2/2]")),
        "validation failure must advance the orchestrator into the next pulse: {:?}",
        *transcript
    );
    assert!(
        workspace.path().join("src/lib.rs").is_file(),
        "write_file pulse must mutate the workspace before validation retries"
    );
    let content =
        std::fs::read_to_string(workspace.path().join("src/lib.rs")).expect("read retried file");
    assert!(
        content.contains("RETRY_ROUND"),
        "mutation pulse must leave a concrete edit behind before validation retries: {content}"
    );
    let validation = edit_loop
        .last_validation_result()
        .expect("last validation result");
    assert!(
        !validation.passed,
        "forced validation failure must remain recorded in edit-loop state"
    );
}

#[tokio::test]
async fn test_edit_loop_run_emits_dirty_workspace_warning_to_transcript() {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    let original_dir = std::env::current_dir().expect("current_dir");
    let workspace = tempfile::tempdir().expect("tempdir");
    fs::write(workspace.path().join("tracked.txt"), "v1\n").expect("seed file");
    run_git(workspace.path(), &["init"]);
    run_git(workspace.path(), &["add", "tracked.txt"]);
    run_git(
        workspace.path(),
        &[
            "-c",
            "user.name=vex-test",
            "-c",
            "user.email=vex-test@example.com",
            "commit",
            "-m",
            "init",
        ],
    );
    fs::write(workspace.path().join("tracked.txt"), "v2\n").expect("mutate file");
    std::env::set_current_dir(workspace.path()).expect("set_current_dir");

    let mut edit_loop = EditLoop::new("task-dirty-warning".to_string()).with_max_turns(1);
    let (mut ctx, mut rx) = make_runtime_context();
    let cancel = CancellationToken::new();
    let outcome = edit_loop
        .run("edit tracked.txt".to_string(), &mut ctx, &cancel)
        .await
        .expect("run should succeed");
    std::env::set_current_dir(original_dir).expect("restore current_dir");

    let warning = rx.recv().await.expect("expected transcript update");
    match warning {
        UiUpdate::TranscriptLine(line) => {
            assert!(
                line.contains("workspace has uncommitted changes"),
                "expected workspace-dirty warning, got: {line}"
            );
        }
        _ => panic!("expected transcript warning update"),
    }
    assert!(matches!(outcome, EditLoopOutcome::MaxTurnsReached { .. }));
}
