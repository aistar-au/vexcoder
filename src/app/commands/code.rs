use super::super::*;

impl TuiMode {
    pub(crate) fn handle_edit_command(&mut self, instruction: &str, ctx: &mut RuntimeContext) {
        if self.active_edit_loop.is_some() && self.history_state.turn_in_progress {
            self.push_history_line(
                "[edit loop already active \u{2014} cancel with Ctrl+C before starting a new task]"
                    .to_string(),
            );
            return;
        }
        if instruction.is_empty() {
            self.push_history_line("[edit] usage: /edit <instruction>".to_string());
            return;
        }
        let instruction = self.expand_slash_instruction_context(instruction);
        self.grant_task_capabilities(
            &[
                Capability::WriteFile,
                Capability::ApplyPatch,
                Capability::RunCommand,
            ],
            "/edit",
        );
        let task_id = self.current_task.id.clone();
        let edit_loop = EditLoop::new(task_id)
            .with_working_dir(self.working_dir.clone())
            .with_profile(self.model_profile.clone());
        self.active_edit_loop = Some(edit_loop.clone());
        self.history_state.active_assistant_index = Some(self.history_state.lines.len() - 1);
        self.history_state.turn_in_progress = true;
        self.set_task_status(TaskStatus::Running);
        #[cfg(test)]
        {
            self.last_turn_input = Some(instruction.clone());
        }
        ctx.start_edit_loop(edit_loop, instruction);
    }
    pub(crate) fn handle_fix_command(&mut self, ctx: &mut RuntimeContext) {
        if self.active_edit_loop.is_some() && self.history_state.turn_in_progress {
            self.push_history_line(
                "[edit loop already active \u{2014} cancel with Ctrl+C before starting a new task]"
                    .to_string(),
            );
            return;
        }
        let last_result = self
            .active_edit_loop
            .as_ref()
            .and_then(|l| l.last_validation_result())
            .filter(|result| !result.passed)
            .cloned();
        let Some(result) = last_result else {
            self.push_history_line(
                "[no recent validation failure in this session \u{2014} run /edit or /test first]"
                    .to_string(),
            );
            return;
        };
        let instruction = result
            .outputs
            .iter()
            .find(|o| o.exit_code != 0)
            .map(|o| format!("fix the {} failure", o.label))
            .unwrap_or_else(|| "fix the validation failure".to_string());
        self.grant_task_capabilities(
            &[
                Capability::WriteFile,
                Capability::ApplyPatch,
                Capability::RunCommand,
            ],
            "/fix",
        );
        let task_id = self.current_task.id.clone();
        let edit_loop = EditLoop::new(task_id)
            .with_working_dir(self.working_dir.clone())
            .with_profile(self.model_profile.clone());
        self.active_edit_loop = Some(edit_loop.clone());
        self.history_state.active_assistant_index = Some(self.history_state.lines.len() - 1);
        self.history_state.turn_in_progress = true;
        self.set_task_status(TaskStatus::Running);
        #[cfg(test)]
        {
            self.last_turn_input = Some(instruction.clone());
        }
        ctx.start_edit_loop(edit_loop, instruction);
    }
    pub(crate) fn handle_explain_command(&mut self, path_hint: &str, ctx: &mut RuntimeContext) {
        let normalized_path_hint = path_hint
            .trim()
            .strip_prefix('@')
            .map(str::trim)
            .filter(|path| !path.is_empty() && !path.contains(char::is_whitespace))
            .unwrap_or(path_hint)
            .trim();
        let requested_path = if !normalized_path_hint.is_empty() {
            Some(normalized_path_hint.to_string())
        } else {
            self.current_task
                .changed_files
                .last()
                .map(|path| path.to_string_lossy().into_owned())
        };
        let scope_instruction = requested_path
            .as_deref()
            .map(|path| format!("explain {path}"))
            .unwrap_or_else(|| "explain the current workspace state".to_string());

        let rendered_context = self.assemble_rendered_context(&scope_instruction);

        let prompt = render_explain_prompt(&scope_instruction, &rendered_context);
        self.start_single_turn(prompt, ctx, true, Some(self.selected_system_prompt()));
    }
    pub(crate) fn handle_review_command(&mut self, args: &str, ctx: &mut RuntimeContext) {
        let parsed = match parse_review_args(args) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.push_history_line(error);
                return;
            }
        };
        let instruction = parsed
            .instruction
            .map(|instruction| self.expand_slash_instruction_context(&instruction))
            .unwrap_or_else(|| {
                "Review these changes for correctness, clarity, and potential issues.".to_string()
            });

        if let Some(files_glob) = parsed.files.as_deref() {
            self.start_review_files_turn(
                files_glob.strip_prefix('@').unwrap_or(files_glob),
                &instruction,
                ctx,
            );
            return;
        }

        let base_ref = parsed.base.as_deref().unwrap_or("HEAD");
        self.start_review_diff_turn(base_ref, &instruction, ctx);
    }

    fn start_review_diff_turn(
        &mut self,
        base_ref: &str,
        instruction: &str,
        ctx: &mut RuntimeContext,
    ) {
        let defaults = ContextAssembler::default();
        let timeout_ms = resolve_git_timeout_ms(defaults.git_timeout_ms);

        match block_on_context_task(run_git_command_with_timeout(
            self.working_dir.clone(),
            vec![
                "rev-parse".to_string(),
                "--verify".to_string(),
                base_ref.to_string(),
            ],
            timeout_ms,
        )) {
            Ok(result) => {
                if result.non_git_repo {
                    self.push_history_line("[review] not a git repository".to_string());
                    return;
                }
                if result.timed_out {
                    self.push_history_line(format!(
                        "[review] error: git rev-parse timed out after {timeout_ms}ms"
                    ));
                    return;
                }
                if result.output.is_none() {
                    self.push_history_line(format!("[review: invalid base ref '{base_ref}']"));
                    return;
                }
            }
            Err(error) => {
                self.push_history_line(format!("[review] error: {error}"));
                return;
            }
        }

        match block_on_context_task(run_git_command_with_timeout(
            self.working_dir.clone(),
            vec!["diff".to_string(), base_ref.to_string()],
            timeout_ms,
        )) {
            Ok(result) => {
                if result.non_git_repo {
                    self.push_history_line("[review] not a git repository".to_string());
                    return;
                }
                if result.timed_out {
                    self.push_history_line(format!(
                        "[review] error: git diff timed out after {timeout_ms}ms"
                    ));
                    return;
                }
                let Some(diff_context) = result.output else {
                    self.push_history_line("[review] error: git diff failed".to_string());
                    return;
                };
                if diff_context.trim().is_empty() {
                    self.push_history_line("[review] working tree is clean".to_string());
                    return;
                }

                let prompt = render_review_prompt(instruction, "", &diff_context);
                self.start_single_turn(prompt, ctx, true, Some(self.selected_system_prompt()));
            }
            Err(error) => {
                self.push_history_line(format!("[review] error: {error}"));
            }
        }
    }

    fn start_review_files_turn(
        &mut self,
        files_glob: &str,
        instruction: &str,
        ctx: &mut RuntimeContext,
    ) {
        let operator = ToolOperator::new(self.working_dir.clone());
        let matched_paths = match operator.find_files(files_glob) {
            Ok(paths) => paths
                .iter()
                .map(|path| operator.to_workspace_relative_display(path))
                .collect::<Vec<_>>(),
            Err(error) => {
                self.push_history_line(format!("[review] error: {error}"));
                return;
            }
        };

        if matched_paths.is_empty() {
            self.push_history_line(format!("[review] no files matched '{files_glob}'"));
            return;
        }

        let assembled = match self.try_assemble_context(&matched_paths.join(" ")) {
            Ok(assembled) => assembled,
            Err(error) => {
                self.push_history_line(format!("[review] error: {error}"));
                return;
            }
        };
        let render_assembler = ContextAssembler::default();
        let diff_context = render_assembler.render(&assembled);
        let mut context_lines = vec![
            format!("[review files] pattern: {files_glob}"),
            format!("[review files] matched: {}", matched_paths.join(", ")),
        ];
        if let Some(status_summary) = assembled
            .git_status_summary
            .as_ref()
            .filter(|summary| !summary.trim().is_empty())
        {
            context_lines.push(status_summary.clone());
        }

        let prompt = render_review_prompt(instruction, &context_lines.join("\n"), &diff_context);
        self.start_single_turn(prompt, ctx, true, Some(self.selected_system_prompt()));
    }

    pub(crate) fn handle_plan_command(&mut self, instruction: &str, ctx: &mut RuntimeContext) {
        if instruction.is_empty() {
            self.push_history_line("[plan] usage: /plan <instruction>".to_string());
            return;
        }
        let instruction = self.expand_slash_instruction_context(instruction);
        let scope_instruction = format!("plan {instruction}");
        let assembled = match self.try_assemble_context(&scope_instruction) {
            Ok(assembled) => assembled,
            Err(error) => {
                self.push_history_line(format!("[plan] error: {error}"));
                return;
            }
        };
        let render_assembler = ContextAssembler::default();
        let rendered_context = render_assembler.render(&assembled);
        let prompt = render_plan_prompt(&instruction, &rendered_context, &scope_instruction);
        self.plan_turn_active = true;
        self.start_single_turn(prompt, ctx, true, Some(self.selected_system_prompt()));
    }
}
