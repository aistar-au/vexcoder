#[cfg(test)]
async fn run_shell_command_with_runner<R, S>(
    runner: R,
    sandbox: S,
    command: String,
    working_dir: PathBuf,
) -> Result<CommandResult>
where
    R: CommandRunner,
    S: SandboxDriver,
{
    let request = sandbox.wrap(shell_command_request(command, working_dir))?;
    runner.run_one_shot(request).await
}

impl Default for TuiMode {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeMode for TuiMode {
    fn on_frontend_signal(&mut self, occurrence: InputOccurrence, ctx: &mut RuntimeContext) {
        match occurrence {
            InputOccurrence::Text(input) => self.on_user_input(input, ctx),
            InputOccurrence::Interrupt => self.on_interrupt(ctx),
            InputOccurrence::Scroll { target, action } => {
                if self.overlay_active() {
                    if target == ScrollTarget::Overlay {
                        self.apply_patch_overlay_scroll_action(action);
                    }
                } else if target == ScrollTarget::Timeline {
                    let total = self.timeline_entry_count();
                    self.apply_timeline_scroll_action(action, total);
                } else if target == ScrollTarget::Output {
                    self.apply_output_scroll_action(action);
                }
            }
        }
    }

    fn on_user_input(&mut self, input: String, ctx: &mut RuntimeContext) {
        TuiMode::on_user_input(self, input, ctx);
    }

    fn on_model_update(&mut self, update: UiUpdate, ctx: &mut RuntimeContext) {
        TuiMode::on_model_update(self, update, ctx);
    }

    fn on_interrupt(&mut self, ctx: &mut RuntimeContext) {
        TuiMode::on_interrupt(self, ctx);
    }

    fn is_pulse_in_progress(&self) -> bool {
        self.task_doc.active_pulse.is_some()
    }
}
