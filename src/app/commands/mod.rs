use super::*;

impl TuiMode {
    fn expand_slash_instruction_context(&self, input: &str) -> String {
        let assembler = ContextAssembler::default();
        let operator = ToolOperator::new(self.working_dir.clone());
        self.expand_inline_tokens_in_text(input, &operator, &assembler)
    }

    fn grant_task_capabilities(&mut self, capabilities: &[Capability], source: &str) {
        let mut granted = Vec::new();
        for &capability in capabilities {
            match self.current_task.active_grants.get(&capability).copied() {
                Some(ApprovalScope::Task) | Some(ApprovalScope::Session) => continue,
                Some(ApprovalScope::Once) | None => {
                    self.current_task
                        .active_grants
                        .insert(capability, ApprovalScope::Task);
                    granted.push(capability_to_kebab(capability));
                }
            }
        }
        if !granted.is_empty() {
            self.push_history_line(format!(
                "[permissions: {source} task grants {}]",
                granted.join(", ")
            ));
        }
    }

    pub(super) fn try_handle_slash_command(
        &mut self,
        input: &str,
        ctx: &mut RuntimeContext,
    ) -> bool {
        if let Some((spec, args)) = Self::registered_slash_command(input) {
            match spec.id {
                SlashCommandId::Quit | SlashCommandId::Exit => self.handle_quit_command(),
                SlashCommandId::About => self.handle_about_command(),
                SlashCommandId::MemoryShow => self.handle_memory_display(),
                SlashCommandId::MemoryAdd => {
                    if args.is_empty() {
                        self.push_history_line("[memory] usage: /memory add <note>".to_string());
                    } else {
                        self.handle_memory_add(args.to_string());
                    }
                }
                SlashCommandId::MemoryClear => {
                    self.overlay_state.pending_memory_clear = true;
                    self.push_history_line(
                        "[memory] clear all notes? type y to confirm or n to cancel".to_string(),
                    );
                }
                SlashCommandId::MemoryAutoOn => self.handle_memory_auto_on(),
                SlashCommandId::MemoryAutoOff => self.handle_memory_auto_off(),
                SlashCommandId::MemoryAutoClear => self.handle_memory_auto_clear(),
                SlashCommandId::New => self.handle_new_command(ctx),
                SlashCommandId::Resume => self.handle_resume_command(args, ctx),
                SlashCommandId::Compact => self.handle_compact_command(ctx),
                SlashCommandId::Fork => self.handle_fork_command(args, ctx),
                SlashCommandId::Permissions => self.handle_permissions_command(),
                SlashCommandId::Allow => self.handle_allow_command(args),
                SlashCommandId::Deny => self.handle_deny_command(args),
                SlashCommandId::Model => self.handle_model_command(args, ctx),
                SlashCommandId::Diff => self.handle_diff_command(args),
                SlashCommandId::Edit => self.handle_edit_command(args, ctx),
                SlashCommandId::Fix => self.handle_fix_command(ctx),
                SlashCommandId::Explain => self.handle_explain_command(args, ctx),
                SlashCommandId::Review => self.handle_review_command(args, ctx),
                SlashCommandId::Plan => self.handle_plan_command(args, ctx),
                SlashCommandId::Init => self.handle_init_command(args),
                SlashCommandId::Run => self.handle_run_command(args),
                SlashCommandId::Test => self.handle_test_command(),
                SlashCommandId::Context => self.handle_context_command(ctx),
                SlashCommandId::Mcp => self.handle_mcp_command(args),
                SlashCommandId::Tools => self.handle_tools_command(args),
                SlashCommandId::Usage => self.handle_usage_command(ctx),
                SlashCommandId::GenerateTests => self.handle_generate_tests_command(args, ctx),
                SlashCommandId::Agents => self.handle_agents_command(),
                SlashCommandId::Delegate => self.handle_delegate_command(args),
                SlashCommandId::Watch => self.handle_watch_command(args),
                SlashCommandId::Undo => self.handle_undo_command(ctx),
                SlashCommandId::Reindex => self.handle_reindex_command(ctx),
                SlashCommandId::Copy => self.handle_copy_command(),
                SlashCommandId::Commands | SlashCommandId::Help => self.handle_commands_command(),
            }

            return true;
        }

        if let Some((command, args)) = self
            .registered_custom_command(input)
            .map(|(command, args)| (command.clone(), args.to_string()))
        {
            self.handle_custom_command(&command, &args, ctx);
            return true;
        }

        false
    }
}

mod code;
mod info;
mod memory;
mod run;
mod session;
