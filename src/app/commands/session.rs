use super::super::*;

impl TuiMode {
    pub(crate) fn handle_custom_command(
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
    pub(crate) fn handle_generate_tests_command(&mut self, args: &str, ctx: &mut RuntimeContext) {
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
    pub(crate) fn default_generate_tests_path(&self) -> Option<String> {
        self.last_assembled_context
            .as_ref()
            .and_then(|context| context.file_rollups.first())
            .map(|snapshot| snapshot.path.to_string_lossy().into_owned())
            .or_else(|| {
                self.current_task
                    .changed_files
                    .last()
                    .map(|path| path.to_string_lossy().into_owned())
            })
    }
    pub(crate) fn infer_generate_tests_framework(&self) -> String {
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
    pub(crate) fn handle_model_command(&mut self, name: &str, ctx: &RuntimeContext) {
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
    /// PK-07: `/diff [--staged]` — show git diff output, capped at 200 lines.
    pub(crate) fn handle_diff_command(&mut self, args: &str) {
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
                        "[diff limited to first {max_diff_lines} lines]"
                    ));
                }
            }
            Err(error) => {
                self.push_history_line(format!("[diff] error: {error}"));
            }
        }
    }
    pub(crate) fn handle_permissions_command(&mut self) {
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
    pub(crate) fn handle_allow_command(&mut self, rest: &str) {
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
    pub(crate) fn handle_deny_command(&mut self, rest: &str) {
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
    pub(crate) fn handle_new_command(&mut self, ctx: &mut RuntimeContext) {
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
    pub(crate) fn handle_resume_command(&mut self, task_id: &str, ctx: &mut RuntimeContext) {
        if task_id.is_empty() {
            let entries = list_recent_task_entries(&self.working_dir, 5);
            if entries.is_empty() {
                self.push_history_line("[resume] no saved tasks found".to_string());
                return;
            }
            self.prompt_resume_selection(entries);
            return;
        }
        match TaskState::load_from_search_dirs_from(&self.working_dir, task_id) {
            Ok(state) => self.apply_resumed_task(state, ctx),
            Err(_) => {
                self.push_history_line(format!("[resume: task '{task_id}' not found]"));
            }
        }
    }
    pub(crate) fn handle_compact_command(&mut self, ctx: &mut RuntimeContext) {
        let task_id = self.current_task.id.clone();
        self.active_edit_loop = None;
        ctx.reset_session_tokens();
        self.current_task.turns.clear();
        self.persist_current_task_state();
        self.reset_conversation_window(ctx);
        self.push_history_line(format!(
            "[compacted: conversation history reset; task {task_id} continues]"
        ));
    }

    pub(crate) fn handle_undo_command(&mut self, ctx: &RuntimeContext) {
        if !ctx.is_undo_enabled() {
            self.push_history_line("[undo] disabled in configuration".to_string());
            return;
        }
        let checkpoint = match ctx.pop_undo_checkpoint() {
            Some(cp) => cp,
            None => {
                self.push_history_line("[undo] nothing to undo".to_string());
                return;
            }
        };
        let display_path = checkpoint
            .path
            .strip_prefix(&self.working_dir)
            .unwrap_or(&checkpoint.path)
            .display();
        if let Some(cleanup_path) = &checkpoint.cleanup_path {
            if cleanup_path.exists() {
                if let Err(e) = std::fs::remove_file(cleanup_path) {
                    let cleanup_display = cleanup_path
                        .strip_prefix(&self.working_dir)
                        .unwrap_or(cleanup_path)
                        .display();
                    self.push_history_line(format!(
                        "[undo] failed to remove {cleanup_display}: {e}"
                    ));
                    return;
                }
            }
        }
        match &checkpoint.previous_content {
            Some(content) => {
                if let Err(e) = std::fs::write(&checkpoint.path, content) {
                    self.push_history_line(format!("[undo] failed to restore {display_path}: {e}"));
                    return;
                }
            }
            None => {
                // File did not exist before the tool call — remove it.
                if checkpoint.path.exists() {
                    if let Err(e) = std::fs::remove_file(&checkpoint.path) {
                        self.push_history_line(format!(
                            "[undo] failed to remove {display_path}: {e}"
                        ));
                        return;
                    }
                }
            }
        }
        let remaining = ctx.undo_stack_len();
        self.push_history_line(format!(
            "[undo] reverted {} on {display_path} ({remaining} checkpoint{} remaining)",
            checkpoint.tool_name,
            if remaining == 1 { "" } else { "s" },
        ));
    }

    pub(crate) fn handle_fork_command(&mut self, label: &str, ctx: &mut RuntimeContext) {
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
    pub(crate) fn handle_quit_command(&mut self) {
        self.quit_requested = true;
    }
    pub(crate) fn handle_about_command(&mut self) {
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

    pub(crate) fn handle_copy_command(&mut self) {
        let text = self
            .history_state
            .lines
            .iter()
            .rev()
            .take(50)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .cloned()
            .collect::<Vec<String>>()
            .join("\n");

        if text.is_empty() {
            self.push_history_line("[copy] nothing to copy".to_string());
            return;
        }

        match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(&text)) {
            Ok(()) => {
                self.push_history_line("[copy] last output copied to clipboard".to_string());
            }
            Err(err) => {
                self.push_history_line(format!("[copy] clipboard unavailable: {err}"));
            }
        }
    }
}
