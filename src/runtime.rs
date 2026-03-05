pub mod approval;
pub mod backend;
pub mod command;
pub mod context;
pub mod context_assembler;
pub mod edit_loop;
pub mod frontend;
pub mod r#loop;
pub mod mode;
pub mod policy;
pub mod task_state;
pub mod text_util;
pub mod update;
pub mod validation;

pub use approval::{
    load_policy_from_env, ApprovalPolicy, ApprovalScope, Capability, FileApprovalPolicy,
    PolicyAction,
};
pub use backend::{ModelBackend, ModelBackendKind, ModelProtocol, ToolCallMode};
pub use command::{
    CommandHandle, CommandRequest, CommandResult, CommandRunner, DefaultCommandRunner, OutputChunk,
    StreamKind,
};
pub use context_assembler::{AssembledContext, ContextAssembler, FileSnapshot};
pub use edit_loop::{EditLoop, EditLoopOutcome};
pub use task_state::{
    CommandEvidence, ConversationCheckpoint, InterruptedCommand, TaskId, TaskState, TaskStatus,
};
pub use text_util::{truncate_head_bytes, truncate_tail_bytes};
pub use update::UiUpdate;
pub use validation::{ValidationCommand, ValidationOutput, ValidationResult, ValidationSuite};

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
