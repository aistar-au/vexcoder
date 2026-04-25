use super::*;
use crate::runtime::tokio::task::spawn_blocking;

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
            PulseToolPolicy::Default,
        );
    }

    pub(super) fn start_single_turn_with_policy(
        &mut self,
        rendered: String,
        ctx: &mut RuntimeContext,
        read_only: bool,
        supplementary_system_prompt: Option<&str>,
        turn_tool_policy: PulseToolPolicy,
    ) {
        self.read_only_turn_active = read_only;
        self.begin_turn_capture_with_policy(rendered.clone(), turn_tool_policy);
        #[cfg(test)]
        {
            self.last_turn_input = Some(rendered.clone());
        }
        ctx.start_pulse_with_system_prompt_and_policy(
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

    pub(super) fn try_assemble_context(
        &mut self,
        scope_instruction: &str,
    ) -> Result<AssembledContext> {
        let assembler = self.context_assembler.clone();
        let operator = ToolOperator::new(self.working_dir.clone());
        let scope_instruction_for_task = scope_instruction.to_string();
        let assembled = block_on_context_task(async move {
            spawn_blocking(move || assembler.assemble(&scope_instruction_for_task, &operator))
                .await
                .map_err(|error| anyhow::anyhow!("failed to join context assembly task: {error}"))?
        })?;
        self.last_assembled_context = Some(assembled.clone());
        Ok(assembled)
    }

    pub(super) fn assemble_rendered_context(&mut self, scope_instruction: &str) -> String {
        let render_assembler = self.context_assembler.clone();
        self.try_assemble_context(scope_instruction)
            .map(|context| render_assembler.render(&context))
            .unwrap_or_else(|_| "## Context\n[context: unavailable]\n".to_string())
    }

    pub(super) fn resolved_notes_path(&self) -> Option<PathBuf> {
        resolve_notes_path_for_write(self.notes_path.as_deref())
    }

    pub(super) fn resolved_existing_notes_path(&self) -> Option<PathBuf> {
        resolve_notes_path_for_read(self.notes_path.as_deref())
    }
}
