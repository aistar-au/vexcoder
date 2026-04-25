use super::*;

pub(super) fn setup_ctx() -> RuntimeContext {
    let (tx, _rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    RuntimeContext::new(conversation, tx, CancellationToken::new())
}
pub(super) fn setup_ctx_with_updates() -> (RuntimeContext, mpsc::UnboundedReceiver<UiUpdate>) {
    let (tx, rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    (
        RuntimeContext::new(conversation, tx, CancellationToken::new()),
        rx,
    )
}
pub(super) fn setup_ctx_with_responses_and_updates(
    responses: Vec<Vec<String>>,
) -> (RuntimeContext, mpsc::UnboundedReceiver<UiUpdate>) {
    let (tx, rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(responses)));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    (
        RuntimeContext::new(conversation, tx, CancellationToken::new()),
        rx,
    )
}
#[test]
fn test_selected_system_prompt_falls_back_to_bundled_prompt() {
    let mut mode = TuiMode::new();
    mode.model_profile.system_prompt = PathBuf::from("src/prompts/missing.txt");

    assert_eq!(mode.selected_system_prompt(), CODER_SYSTEM_PROMPT);
}
pub(super) fn setup_ctx_with_responses(responses: Vec<Vec<String>>) -> RuntimeContext {
    let (tx, _rx) = mpsc::unbounded_channel::<UiUpdate>();
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(responses)));
    let conversation = ConversationManager::new_mock(client, HashMap::new());
    RuntimeContext::new(conversation, tx, CancellationToken::new())
}
pub(super) fn config_with_workdir(path: &std::path::Path) -> Config {
    let mut config = Config::default_for_tui();
    config.working_dir = path.to_path_buf();
    config
}
pub(super) fn write_custom_command(
    dir: &std::path::Path,
    file_name: &str,
    name: &str,
    description: &str,
    template: &str,
) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join(file_name),
        format!("name = {name:?}\ndescription = {description:?}\ntemplate = {template:?}\n"),
    )
    .unwrap();
}
pub(super) fn successful_run_input() -> String {
    if cfg!(windows) {
        "/run cmd /C exit 0".to_string()
    } else {
        "/run sh -c true".to_string()
    }
}
pub(super) fn successful_bang_input() -> String {
    "!echo inline-shell".to_string()
}
pub(super) async fn drain_until_turn_complete(
    mode: &mut TuiMode,
    ctx: &mut RuntimeContext,
    rx: &mut mpsc::UnboundedReceiver<UiUpdate>,
) {
    loop {
        let update = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("timed out waiting for ui update")
            .expect("ui update channel closed");
        let is_final_update = matches!(update, UiUpdate::PulseComplete | UiUpdate::Error(_));
        mode.on_model_update(update, ctx);
        if is_final_update && !mode.is_pulse_in_progress() {
            break;
        }
    }
}
#[derive(Clone)]
pub(super) struct RecordingSandbox {
    pub(super) wrapped: Arc<AtomicBool>,
}
impl SandboxDriver for RecordingSandbox {
    fn wrap(&self, request: CommandRequest) -> Result<CommandRequest> {
        self.wrapped.store(true, Ordering::SeqCst);
        Ok(request)
    }
}
pub(super) fn init_git_repo(path: &std::path::Path) {
    git_success(path, &["init"]);
    git_success(path, &["config", "user.name", "test"]);
    git_success(path, &["config", "user.email", "t@t"]);
}
pub(super) fn git_success(path: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: stdout={} stderr={}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
pub(super) async fn wait_for_model_turn(ctx: &RuntimeContext, label: &str) {
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if ctx.test_message_count().await > 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{label} must start a single model pulse"));
}
