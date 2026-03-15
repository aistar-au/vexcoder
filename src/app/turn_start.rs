use super::*;

impl TuiMode {
    pub(super) fn start_single_turn(
        &mut self,
        rendered: String,
        ctx: &mut RuntimeContext,
        read_only: bool,
        supplementary_system_prompt: Option<&str>,
    ) {
        self.start_single_turn_with_policy(
            rendered,
            ctx,
            read_only,
            supplementary_system_prompt,
            TurnToolPolicy::Default,
        );
    }

    pub(super) fn start_single_turn_with_policy(
        &mut self,
        rendered: String,
        ctx: &mut RuntimeContext,
        read_only: bool,
        supplementary_system_prompt: Option<&str>,
        turn_tool_policy: TurnToolPolicy,
    ) {
        self.history_state.active_assistant_index = Some(self.history_state.lines.len() - 1);
        self.history_state.turn_in_progress = true;
        self.read_only_turn_active = read_only;
        self.begin_turn_capture(rendered.clone());
        #[cfg(test)]
        {
            self.last_turn_input = Some(rendered.clone());
        }
        ctx.start_turn_with_system_prompt_and_policy(
            rendered,
            supplementary_system_prompt.map(ToString::to_string),
            turn_tool_policy,
        );
    }

    pub(super) fn selected_system_prompt(&self) -> &'static str {
        self.model_profile
            .system_prompt_text()
            .unwrap_or(CODER_SYSTEM_PROMPT)
    }

    pub(super) fn assemble_rendered_context(&mut self, scope_instruction: &str) -> String {
        let assembler = ContextAssembler::default();
        let render_assembler = assembler.clone();
        let operator = ToolOperator::new(self.working_dir.clone());
        let scope_instruction_for_task = scope_instruction.to_string();
        let assembled = block_on_context_task(async move {
            tokio::task::spawn_blocking(move || {
                assembler.assemble(&scope_instruction_for_task, &operator)
            })
            .await
            .map_err(|error| anyhow::anyhow!("failed to join context assembly task: {error}"))?
        })
        .ok();
        if let Some(context) = assembled.clone() {
            self.last_assembled_context = Some(context);
        }
        assembled
            .as_ref()
            .map(|context| render_assembler.render(context))
            .unwrap_or_else(|| "## Context\n[context: unavailable]\n".to_string())
    }

    pub(super) fn resolved_notes_path(&self) -> Option<PathBuf> {
        resolve_notes_path_for_write(self.notes_path.as_deref())
    }

    pub(super) fn resolved_existing_notes_path(&self) -> Option<PathBuf> {
        resolve_notes_path_for_read(self.notes_path.as_deref())
    }
}
