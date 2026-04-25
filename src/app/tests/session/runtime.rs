use super::*;
use crate::config::CompactionConfig;
use crate::config::UndoConfig;

fn make_config(temp: &std::path::Path) -> Config {
    Config {
        model_token: None,
        model_name: "mock-model".to_string(),
        model_url: "http://localhost:8000/v1/messages".to_string(),
        model_url_skip_tls_check: false,
        working_dir: temp.to_path_buf(),
        model_backend: crate::runtime::ModelBackendKind::LocalRuntime,
        model_protocol: crate::runtime::ModelProtocol::MessagesV1,
        tool_call_mode: crate::runtime::ToolCallMode::Structured,
        tool_policy: crate::runtime::ToolPolicy::Full,
        model_profile: ModelProfile::default_for_backend(crate::runtime::ModelBackendKind::LocalRuntime),
        max_project_instructions_tokens: 4096,
        max_memory_tokens: 2048,
        sandbox: crate::runtime::SandboxConfig::default(),
        model_headers: reqwest::header::HeaderMap::new(),
        mcp_servers: Vec::new(),
        http_hooks: Vec::new(),
        compaction: CompactionConfig::default(),
        undo: UndoConfig::default(),
        search: crate::config::SearchConfig { auto_index: false, ..Default::default() },
        notes_path: None,
        api: crate::config::ApiConfig::default(),
        hooks: Vec::new(),
        auto_memory: crate::config::AutoMemoryConfig::default(),
        api_client: crate::config::ApiClientConfig::default(),
        force: false,
        bypass_policy: false,
        expand_context: false,
    }
}

#[test]
fn build_runtime_with_resume_restores_task_and_grants() {
    let temp = tempfile::tempdir().unwrap();
    let mut state = TaskState::new("task-startup-resume".to_string());
    state.active_grants.insert(Capability::Network, ApprovalScope::Session);
    state.status = crate::runtime::TaskStatus::Running;

    let (runtime, _ctx) = build_runtime_with_resume(make_config(temp.path()), state).unwrap();
    assert_eq!(runtime.mode.task_doc.info.id, "task-startup-resume");
    assert_eq!(runtime.mode.task_doc.info.active_grants.get(&Capability::Network), Some(&ApprovalScope::Session));
}

#[test]
fn compact_resets_turn_evidence_and_preserves_task_id() {
    let temp = tempfile::tempdir().unwrap();
    let mut mode = TuiMode::new();
    let original_id = mode.current_task_id();
    let mut ctx = setup_ctx();
    mode.push_history_line("turn1".to_string());
    mode.on_user_input("/compact".to_string(), &mut ctx);
    assert_eq!(mode.current_task_id(), original_id, "compact must not change task-id");
    assert!(mode.active_edit_loop.is_none(), "compact must clear edit loop");
}
