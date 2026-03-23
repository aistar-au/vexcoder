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
            let previous = self
                .current_task
                .active_grants
                .insert(capability, ApprovalScope::Task);
            if previous != Some(ApprovalScope::Task) {
                granted.push(capability_to_kebab(capability));
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
                SlashCommandId::New => self.handle_new_command(ctx),
                SlashCommandId::Resume => self.handle_resume_command(args, ctx),
                SlashCommandId::Clear => self.handle_clear_command(ctx),
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
                SlashCommandId::Tools => self.handle_tools_command(args),
                SlashCommandId::Usage => self.handle_usage_command(ctx),
                SlashCommandId::GenerateTests => self.handle_generate_tests_command(args, ctx),
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
    pub(super) fn handle_edit_command(&mut self, instruction: &str, ctx: &mut RuntimeContext) {
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
        #[cfg(test)]
        {
            self.last_turn_input = Some(instruction.clone());
        }
        ctx.start_edit_loop(edit_loop, instruction);
    }
    pub(super) fn handle_fix_command(&mut self, ctx: &mut RuntimeContext) {
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
        #[cfg(test)]
        {
            self.last_turn_input = Some(instruction.clone());
        }
        ctx.start_edit_loop(edit_loop, instruction);
    }
    pub(super) fn handle_explain_command(&mut self, path_hint: &str, ctx: &mut RuntimeContext) {
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
    pub(super) fn handle_review_command(&mut self, args: &str, ctx: &mut RuntimeContext) {
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

    pub(super) fn handle_plan_command(&mut self, instruction: &str, ctx: &mut RuntimeContext) {
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
        self.start_single_turn(prompt, ctx, true, Some(self.selected_system_prompt()));
    }
    pub(super) fn handle_init_command(&mut self, environment: &str) {
        match crate::init::scaffold_workspace(&self.working_dir) {
            Ok(summary) => {
                for line in summary {
                    self.push_history_line(line);
                }
                if !environment.trim().is_empty() {
                    self.push_history_line(format!(
                        "[init] selected environment: {}",
                        environment.trim()
                    ));
                }
            }
            Err(error) => {
                self.push_history_line(format!("[init] error: {error}"));
            }
        }
    }
    pub(super) fn handle_run_command(&mut self, command_str: &str) {
        let suite = if command_str.is_empty() {
            let mut inferred = ValidationSuite::load_or_infer(&self.working_dir);
            inferred.commands.truncate(1);
            inferred
        } else {
            let mut parts = command_str.split_whitespace();
            let Some(program) = parts.next() else {
                self.push_history_line("[run] usage: /run [command]".to_string());
                return;
            };
            ValidationSuite {
                commands: vec![crate::runtime::ValidationCommand {
                    label: command_str.to_string(),
                    program: program.to_string(),
                    args: parts.map(ToString::to_string).collect(),
                    timeout_secs: 60,
                }],
            }
        };

        self.run_validation_suite_to_transcript(suite, "run", false);
    }
    pub(super) fn handle_test_command(&mut self) {
        let suite = ValidationSuite::load_or_infer(&self.working_dir);
        self.run_validation_suite_to_transcript(suite, "test", true);
    }
    pub(super) fn run_validation_suite_to_transcript(
        &mut self,
        suite: ValidationSuite,
        label: &str,
        remember_for_fix: bool,
    ) {
        if suite.commands.is_empty() {
            self.push_history_line(format!("[{label}] no commands configured"));
            return;
        }

        match block_on_context_task(run_validation_suite_capture(
            suite,
            self.working_dir.clone(),
        )) {
            Ok(result) => {
                if remember_for_fix {
                    let mut edit_loop = self.active_edit_loop.clone().unwrap_or_else(|| {
                        EditLoop::new(self.current_task.id.clone())
                            .with_working_dir(self.working_dir.clone())
                            .with_profile(self.model_profile.clone())
                    });
                    edit_loop.set_last_validation_result(result.clone());
                    self.active_edit_loop = Some(edit_loop);
                }
                self.push_validation_result_lines(label, &result);
            }
            Err(error) => {
                self.push_history_line(format!("[{label}] error: {error}"));
            }
        }
    }
    pub(super) fn push_validation_result_lines(
        &mut self,
        label: &str,
        result: &crate::runtime::ValidationResult,
    ) {
        for output in &result.outputs {
            let status = if output.exit_code == 0 {
                "ok".to_string()
            } else {
                format!("exit {}", output.exit_code)
            };
            self.push_history_line(format!("[{label}] {} [{status}]", output.label));
            if !output.stdout_tail.trim().is_empty() {
                for line in output.stdout_tail.lines() {
                    self.push_history_line(format!("  stdout: {line}"));
                }
                if output.stdout_truncated {
                    self.push_history_line("  stdout: [truncated]".to_string());
                }
            }
            if !output.stderr_tail.trim().is_empty() {
                for line in output.stderr_tail.lines() {
                    self.push_history_line(format!("  stderr: {line}"));
                }
                if output.stderr_truncated {
                    self.push_history_line("  stderr: [truncated]".to_string());
                }
            }
            if output.stdout_tail.trim().is_empty() && output.stderr_tail.trim().is_empty() {
                self.push_history_line("  output: [no captured output]".to_string());
            }
        }

        let summary = if result.passed {
            "all commands passed"
        } else {
            "one or more commands failed"
        };
        self.push_history_line(format!("[{label}] {summary}"));
    }
    pub(super) fn handle_context_command(&mut self, ctx: &RuntimeContext) {
        let turns = if self.active_edit_loop.is_some() && self.history_state.turn_in_progress {
            "1".to_string()
        } else {
            "\u{2014}".to_string()
        };
        let profile_name = self
            .active_edit_loop
            .as_ref()
            .map(|edit_loop| edit_loop.profile_name())
            .unwrap_or(self.model_profile.name.as_str())
            .to_string();
        let files = self
            .last_assembled_context
            .as_ref()
            .map(|context| context.file_snapshots.len())
            .unwrap_or(0);
        let git_summary = self.resolve_context_git_summary();

        self.push_history_line("[context]".to_string());
        self.push_history_line(format!("  model     : {}", self.model_name));
        self.push_history_line(format!("  backend   : {:?}", self.model_backend));
        self.push_history_line(format!("  profile   : {profile_name}"));
        self.push_history_line(format!("  task      : {}", self.current_task.id));
        self.push_history_line(format!("  status    : {:?}", self.current_task.status));
        self.push_history_line(format!("  turns     : {turns}"));
        self.push_history_line(format!("  files     : {files}"));
        self.push_history_line(format!("  git       : {git_summary}"));
        self.push_history_line(format!(
            "  approvals : {} active grant(s)",
            self.current_task.active_grants.len()
        ));
        self.push_history_line(format!(
            "  tokens    : ~{}",
            ctx.estimated_conversation_tokens()
        ));
    }
    pub(super) fn resolve_context_git_summary(&self) -> String {
        let defaults = ContextAssembler::default();
        let timeout_ms = resolve_git_timeout_ms(defaults.git_timeout_ms);
        match block_on_context_task(run_git_command_with_timeout(
            self.working_dir.clone(),
            vec!["status".to_string(), "--short".to_string()],
            timeout_ms,
        )) {
            Ok(result) => {
                if result.non_git_repo {
                    "no git".to_string()
                } else if result.timed_out {
                    "timed out".to_string()
                } else {
                    result
                        .output
                        .and_then(|text| {
                            let first = text.lines().next().unwrap_or("clean").trim().to_string();
                            (!first.is_empty()).then_some(first)
                        })
                        .unwrap_or_else(|| "clean".to_string())
                }
            }
            Err(_) => "no git".to_string(),
        }
    }
    pub(super) fn handle_commands_command(&mut self) {
        let mut seen = std::collections::HashSet::new();
        self.push_history_line("[commands]".to_string());
        for spec in SLASH_COMMANDS {
            if seen.insert(spec.display) {
                self.push_history_line(format!("  {:32} — {}", spec.display, spec.description));
            }
        }
        if !self.custom_commands.is_empty() {
            self.push_history_line("[custom commands]".to_string());
            let custom_command_lines = self
                .custom_commands
                .iter()
                .map(|command| format!("  {:32} — {}", command.display(), command.description))
                .collect::<Vec<_>>();
            for line in custom_command_lines {
                self.push_history_line(line);
            }
        }
    }
    pub(super) fn handle_tools_command(&mut self, args: &str) {
        let include_descriptions = match args.trim() {
            "" => false,
            "desc" => true,
            _ => {
                self.push_history_line("[tools] usage: /tools [desc]".to_string());
                return;
            }
        };

        self.push_history_line("[tools]".to_string());
        self.push_history_line(
            "[tools] MCP registry not yet available; built-in tools only".to_string(),
        );
        for tool in builtin_tool_summaries() {
            if include_descriptions {
                self.push_history_line(format!("  {:24} — {}", tool.name, tool.description));
            } else {
                self.push_history_line(format!("  {}", tool.name));
            }
        }
    }
    pub(super) fn handle_usage_command(&mut self, ctx: &RuntimeContext) {
        let usage = ctx.session_tokens_snapshot();
        if !usage.has_completed_turns() {
            self.push_history_line("[usage] no turns completed this session".to_string());
            return;
        }

        let turn_estimated = Self::summarize_usage_line_suffix(usage.last_estimated);
        let session_estimated = Self::summarize_usage_line_suffix(usage.estimated);
        self.push_history_line("[usage]".to_string());
        self.push_history_line(format!(
            "  this turn   : {} in / {} out{}",
            usage.last_input, usage.last_output, turn_estimated
        ));
        self.push_history_line(format!(
            "  session     : {} in / {} out{}",
            usage.input, usage.output, session_estimated
        ));
    }
    pub(super) fn handle_custom_command(
        &mut self,
        command: &CustomCommand,
        args: &str,
        ctx: &mut RuntimeContext,
    ) {
        let scope_instruction = if args.is_empty() {
            format!("run custom command {}", command.name)
        } else {
            args.to_string()
        };
        let rendered_context = self.assemble_rendered_context(&scope_instruction);
        let instruction =
            render_custom_command_instruction(&command.template, &rendered_context, args);
        let prompt = if command.template.contains("{{context}}") {
            instruction
        } else {
            render_edit_prompt(&instruction, &rendered_context)
        };
        self.start_single_turn(prompt, ctx, false, None);
    }
    pub(super) fn handle_generate_tests_command(&mut self, args: &str, ctx: &mut RuntimeContext) {
        let parsed = match parse_generate_tests_args(args) {
            Ok(parsed) => parsed,
            Err(message) => {
                self.push_history_line(message);
                return;
            }
        };
        let Some(target_path) = parsed.path.or_else(|| self.default_generate_tests_path()) else {
            self.push_history_line(
                "[generate-tests] usage: /generate-tests [path] [--framework <name>]".to_string(),
            );
            return;
        };

        let scope_instruction = format!("generate tests for {target_path}");
        let rendered_context = self.assemble_rendered_context(&scope_instruction);
        let framework = parsed
            .framework
            .unwrap_or_else(|| self.infer_generate_tests_framework());
        let prompt =
            render_generate_tests_prompt(&scope_instruction, &rendered_context, &framework);
        self.start_single_turn_with_policy(
            prompt,
            ctx,
            false,
            Some(self.selected_system_prompt()),
            TurnToolPolicy::TestsOnlyMutations,
        );
    }
    pub(super) fn default_generate_tests_path(&self) -> Option<String> {
        self.last_assembled_context
            .as_ref()
            .and_then(|context| context.file_snapshots.first())
            .map(|snapshot| snapshot.path.to_string_lossy().into_owned())
            .or_else(|| {
                self.current_task
                    .changed_files
                    .last()
                    .map(|path| path.to_string_lossy().into_owned())
            })
    }
    pub(super) fn infer_generate_tests_framework(&self) -> String {
        ValidationSuite::infer_from_repo(&self.working_dir)
            .commands
            .first()
            .map(|command| match command.program.as_str() {
                "cargo" => "cargo-test".to_string(),
                "npm" => "npm-test".to_string(),
                "make" => "make-test".to_string(),
                other if !other.trim().is_empty() => other.trim().to_string(),
                _ => "project-tests".to_string(),
            })
            .unwrap_or_else(|| "project-tests".to_string())
    }
    /// PC-01: `/model <n>` — name-only switch within the same backend/protocol.
    pub(super) fn handle_model_command(&mut self, name: &str, ctx: &RuntimeContext) {
        if name.is_empty() {
            self.push_history_line(format!("[model] {}", self.model_name));
            return;
        }
        // Models prefixed with `local/` are local-runtime-only; all other
        // names are assumed compatible with the API backend. Reject any
        // name that would require switching backends mid-session.
        let target_is_local = name.starts_with("local/");
        let current_is_local = self.model_backend == crate::runtime::ModelBackendKind::LocalRuntime;

        if target_is_local != current_is_local {
            let required_backend = if target_is_local {
                "local-runtime"
            } else {
                "api-server"
            };
            self.push_history_line(format!(
                "[model] rejected: '{}' requires {} backend \
                 (current: {:?}). Restart vex with the desired backend.",
                name, required_backend, self.model_backend,
            ));
            return;
        }

        if let Err(error) = ctx.set_model_name(name.to_string()) {
            self.push_history_line(format!("[model] error: {error}"));
            return;
        }

        let old = std::mem::replace(&mut self.model_name, name.to_string());
        self.push_history_line(format!("[model] {} -> {}", old, self.model_name));
    }
    /// PK-07: `/diff [--staged]` — show git diff output, truncated at 200 lines.
    pub(super) fn handle_diff_command(&mut self, args: &str) {
        let diff_defaults = ContextAssembler::default();
        let max_diff_lines = diff_defaults.max_diff_lines;
        let timeout_ms = resolve_git_timeout_ms(diff_defaults.git_timeout_ms);
        let staged = match args.split_whitespace().collect::<Vec<_>>().as_slice() {
            [] => false,
            ["--staged"] | ["--cached"] => true,
            _ => {
                self.push_history_line("[diff] usage: /diff [--staged]".to_string());
                return;
            }
        };

        let git_args = if staged {
            vec!["diff".to_string(), "--cached".to_string()]
        } else {
            vec!["diff".to_string(), "HEAD".to_string()]
        };

        match block_on_context_task(run_git_command_with_timeout(
            self.working_dir.clone(),
            git_args,
            timeout_ms,
        )) {
            Ok(result) => {
                if result.non_git_repo {
                    self.push_history_line("[diff] not a git repository".to_string());
                    return;
                }
                if result.timed_out {
                    self.push_history_line(format!(
                        "[diff] error: git diff timed out after {timeout_ms}ms"
                    ));
                    return;
                }
                let Some(text) = result.output else {
                    self.push_history_line("[diff] error: git diff failed".to_string());
                    return;
                };
                if text.trim().is_empty() {
                    self.push_history_line("[diff] working tree is clean".to_string());
                    return;
                }

                let lines: Vec<&str> = text.lines().collect();
                for line in lines.iter().take(max_diff_lines) {
                    self.push_history_line(line.to_string());
                }
                if lines.len() > max_diff_lines {
                    self.push_history_line(format!(
                        "[diff truncated \u{2014} showing first {max_diff_lines} lines]"
                    ));
                }
            }
            Err(error) => {
                self.push_history_line(format!("[diff] error: {error}"));
            }
        }
    }
    pub(super) fn handle_permissions_command(&mut self) {
        self.push_history_line("[permissions]".to_string());
        for &cap in ALL_CAPABILITIES {
            let cap_name = capability_to_kebab(cap);
            let scope_label = self
                .current_task
                .active_grants
                .get(&cap)
                .map(|scope| scope_to_label(*scope))
                .unwrap_or("(none)");
            self.push_history_line(format!("  {cap_name}  {scope_label}"));
        }
    }
    pub(super) fn handle_allow_command(&mut self, rest: &str) {
        if rest.is_empty() {
            self.push_history_line(
                "[allow: usage: /allow <capability> [once|session]]".to_string(),
            );
            return;
        }
        let mut parts = rest.splitn(2, ' ');
        let cap_str = parts.next().unwrap_or("").trim();
        let scope_str = parts.next().unwrap_or("").trim();

        let Some(cap) = kebab_to_capability(cap_str) else {
            self.push_history_line(format!("[allow: unknown capability '{cap_str}']"));
            return;
        };

        let scope = if scope_str.is_empty() {
            ApprovalScope::Once
        } else {
            match kebab_to_scope(scope_str) {
                Some(s) => s,
                None => {
                    self.push_history_line(format!(
                        "[allow: unknown scope '{scope_str}'; valid: once | session]"
                    ));
                    return;
                }
            }
        };

        let scope_label = scope_to_label(scope);
        self.current_task.active_grants.insert(cap, scope);
        self.push_history_line(format!("[allow: {cap_str} granted for {scope_label}]"));
    }
    pub(super) fn handle_deny_command(&mut self, rest: &str) {
        if rest.is_empty() {
            self.push_history_line("[deny: usage: /deny <capability>]".to_string());
            return;
        }
        let cap_str = rest.trim();
        let Some(cap) = kebab_to_capability(cap_str) else {
            self.push_history_line(format!("[deny: unknown capability '{cap_str}']"));
            return;
        };

        if self.current_task.active_grants.remove(&cap).is_some() {
            self.push_history_line(format!("[deny: {cap_str} removed]"));
        } else {
            self.push_history_line(format!("[deny: {cap_str} not in active grants]"));
        }
    }
    pub(super) fn handle_new_command(&mut self, ctx: &mut RuntimeContext) {
        let dir = TaskState::state_dir();
        if let Err(e) = self.current_task.save(&dir) {
            self.push_history_line(format!("[new] save failed: {e} - session not reset"));
            return;
        }
        let new_id = new_task_id();
        self.current_task = TaskState::new(new_id.clone());
        self.current_task.instructions_path = self.instructions_path.clone();
        self.active_edit_loop = None;
        ctx.reset_session_tokens();
        self.reset_conversation_window(ctx);
        self.push_history_line(format!("[new session: {new_id}]"));
    }
    pub(super) fn handle_resume_command(&mut self, task_id: &str, ctx: &mut RuntimeContext) {
        if task_id.is_empty() {
            let entries = list_recent_task_entries(5);
            if entries.is_empty() {
                self.push_history_line("[resume] no saved tasks found".to_string());
                return;
            }
            self.prompt_resume_selection(entries);
            return;
        }
        match TaskState::load_from_search_dirs(task_id) {
            Ok(state) => self.apply_resumed_task(state, ctx),
            Err(_) => {
                self.push_history_line(format!("[resume: task '{task_id}' not found]"));
            }
        }
    }
    pub(super) fn handle_clear_command(&mut self, ctx: &mut RuntimeContext) {
        let task_id = self.current_task.id.clone();
        self.active_edit_loop = None;
        ctx.reset_session_tokens();
        self.reset_conversation_window(ctx);
        self.push_history_line(format!(
            "[cleared: conversation history reset; task {task_id} continues]"
        ));
    }
    pub(super) fn handle_fork_command(&mut self, label: &str, ctx: &mut RuntimeContext) {
        let dir = TaskState::state_dir();
        if let Err(e) = self.current_task.save(&dir) {
            self.push_history_line(format!("[fork] save failed: {e} - fork aborted"));
            return;
        }
        let sanitized_label = sanitize_task_label(label);
        let new_id = if sanitized_label.is_empty() {
            format!("{}-fork", new_task_id())
        } else {
            format!("{}-{sanitized_label}", new_task_id())
        };
        let parent_id = self.current_task.id.clone();
        let mut fork = TaskState::new(new_id.clone());
        fork.active_grants = self.current_task.active_grants.clone();
        fork.changed_files = self.current_task.changed_files.clone();
        fork.status = self.current_task.status.clone();
        fork.instructions_path = self.instructions_path.clone();
        self.current_task = fork;
        self.reset_conversation_window(ctx);
        self.push_history_line(format!("[fork: {new_id} branched from {parent_id}]"));
    }
    pub(super) fn handle_quit_command(&mut self) {
        self.quit_requested = true;
    }
    pub(super) fn handle_about_command(&mut self) {
        let version = env!("CARGO_PKG_VERSION");
        let commit = env!("GIT_COMMIT_SHORT");
        let build_date = env!("BUILD_DATE");
        self.push_history_line(format!("vex {version}"));
        self.push_history_line(format!("  build     : {build_date}"));
        self.push_history_line(format!("  commit    : {commit}"));
        self.push_history_line(format!("  repo      : {}", self.repo_label));
        self.push_history_line(format!(
            "  inst      : {}",
            self.instructions_path.as_deref().unwrap_or("none")
        ));
    }
    pub(super) fn handle_memory_display(&mut self) {
        let content = self
            .resolved_existing_notes_path()
            .and_then(|path| std::fs::read_to_string(path).ok());
        match content {
            Some(content) if !content.trim().is_empty() => {
                for line in content.lines() {
                    self.push_history_line(line.to_string());
                }
            }
            _ => {
                self.push_history_line("[memory] no notes".to_string());
            }
        }
    }
    pub(super) fn handle_memory_add(&mut self, note: String) {
        if note.is_empty() {
            self.push_history_line("[memory] usage: /memory add <note>".to_string());
            return;
        }
        let path = self
            .resolved_existing_notes_path()
            .or_else(|| self.resolved_notes_path());
        let Some(path) = path else {
            self.push_history_line("[memory] error resolving notes path".to_string());
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            Ok(mut f) => {
                if let Err(e) = writeln!(f, "{}", note) {
                    self.push_history_line(format!("[memory] error writing: {e}"));
                    return;
                }
                self.push_history_line("[memory: note added]".to_string());
            }
            Err(e) => {
                self.push_history_line(format!("[memory] error opening file: {e}"));
            }
        }
    }
    pub(super) fn handle_memory_clear_input(&mut self, input: &str) {
        self.overlay_state.pending_memory_clear = false;
        match input.trim().to_lowercase().as_str() {
            "y" | "yes" => {
                let path = self
                    .resolved_existing_notes_path()
                    .or_else(|| self.resolved_notes_path());
                let Some(path) = path else {
                    self.push_history_line("[memory] error resolving notes path".to_string());
                    return;
                };
                if path.exists() {
                    if let Err(e) = std::fs::write(&path, "") {
                        self.push_history_line(format!("[memory] error clearing: {e}"));
                        return;
                    }
                }
                self.push_history_line("[memory: cleared]".to_string());
            }
            _ => {
                self.push_history_line("[memory: cancelled]".to_string());
            }
        }
    }
}
