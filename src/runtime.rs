pub mod approval;
pub mod backend;
pub mod command;
pub mod context;
pub mod context_assembler;
pub mod context_cache;
pub mod edit_loop;
pub mod frontend;
pub mod git_parse;
pub mod git_rollup;
pub mod json_handoff;
pub mod r#loop;
pub mod mode;
pub mod policy;
pub mod project_instructions;
pub mod rate_limit;
pub mod sandbox;
pub mod secrets;
pub mod session_task;
pub mod task_state;
pub mod text_util;
pub mod update;
pub mod validation;
pub mod worktree_lease;

pub use approval::{
    load_policy_from_env, ApprovalPolicy, ApprovalScope, Capability, FileApprovalPolicy,
    PolicyAction,
};
pub use backend::{ModelBackend, ModelBackendKind, ModelProtocol, ToolCallMode};
pub(crate) use command::{
    format_command_session_cancelled, format_command_session_exit, format_command_session_output,
    format_command_session_started,
};
pub use command::{
    CommandHandle, CommandRequest, CommandResult, CommandRunner, DefaultCommandRunner, OutputChunk,
    StreamKind,
};
pub use context_assembler::{AssembledContext, ContextAssembler, FileRollup};
pub use edit_loop::{EditLoop, EditLoopOutcome};
pub use git_parse::{
    parse_diff_stat, parse_git_apply, parse_git_status, ApplyOutcome, DiffStatEntry,
    ParsedDiffStat, ParsedGitApply, ParsedGitStatus, StatusEntry,
};
pub(crate) use git_rollup::{
    block_on_context_task, resolve_git_timeout_ms, run_git_command_with_timeout,
};
pub use json_handoff::{
    RuntimeEnvelope, RuntimeEvent, RuntimeRequest, TokenUsageEnvelope, ValidationOutputEnvelope,
};
pub use sandbox::{
    resolve_configured_sandbox, ConfiguredSandbox, PassthroughSandbox, SandboxConfig,
    SandboxDriver, SandboxKind,
};
pub use session_task::{SessionTask, SessionTaskId, SessionTaskStatus};
pub use task_state::{
    CacheUsageStats, CommandEvidence, ContextCompactionRecord, ConversationCheckpoint,
    InterruptedCommand, TaskId, TaskState, TaskStatus,
};
pub use text_util::{truncate_head_bytes, truncate_tail_bytes};
pub use rate_limit::{
    looks_like_rate_limit, parse_retry_after_header, parse_retry_from_body, RetryHint,
    RetryHintSource,
};
pub use secrets::{contains_secret, redact_secrets};
pub use update::UiUpdate;
pub use validation::{ValidationCommand, ValidationOutput, ValidationResult, ValidationSuite};
pub use worktree_lease::{WorktreeLease, WorktreeLeaseManager};

#[cfg(test)]
mod tests {
    #[test]
    fn test_ref_02_runtime_types_compile() {
        use crate::runtime::{
            context::RuntimeContext,
            frontend::{FrontendAdapter, UserInputEvent},
            mode::RuntimeMode,
        };

        fn _uses_runtime_mode_trait<T: RuntimeMode>() {}
        fn _uses_frontend_adapter_trait<T: FrontendAdapter<DummyMode>>() {}

        struct DummyMode;
        impl RuntimeMode for DummyMode {
            fn on_user_input(&mut self, _input: String, _ctx: &mut RuntimeContext) {}
            fn on_model_update(
                &mut self,
                _update: crate::runtime::UiUpdate,
                _ctx: &mut RuntimeContext,
            ) {
            }
            fn is_turn_in_progress(&self) -> bool {
                false
            }
        }

        struct DummyFrontend;
        impl FrontendAdapter<DummyMode> for DummyFrontend {
            fn poll_user_input(&mut self, _mode: &DummyMode) -> Option<UserInputEvent> {
                None
            }
            fn render(&mut self, _mode: &DummyMode) {}
            fn should_quit(&self) -> bool {
                true
            }
        }

        let _ = std::mem::size_of::<Option<RuntimeContext>>();
        let _ = _uses_runtime_mode_trait::<DummyMode>;
        let _ = _uses_frontend_adapter_trait::<DummyFrontend>;
    }

    #[test]
    fn test_is_local_endpoint_url_normalizes_case_and_space() {
        assert!(crate::util::is_local_endpoint_url(
            " HTTP://LOCALHOST:8000/v1/messages "
        ));
        assert!(crate::util::is_local_endpoint_url(
            "https://127.0.0.1/v1/messages"
        ));
        assert!(!crate::util::is_local_endpoint_url(
            "https://api.example.com/v1/messages"
        ));
    }
}
