use super::super::*;

impl TuiMode {
    pub(crate) fn handle_init_command(&mut self, environment: &str) {
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
    pub(crate) fn handle_run_command(&mut self, command_str: &str) {
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
    pub(crate) fn handle_test_command(&mut self) {
        let suite = ValidationSuite::load_or_infer(&self.working_dir);
        self.run_validation_suite_to_transcript(suite, "test", true);
    }
    pub(crate) fn run_validation_suite_to_transcript(
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
            self.sandbox.clone(),
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
    pub(crate) fn push_validation_result_lines(
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
    pub(crate) fn handle_context_command(&mut self, ctx: &RuntimeContext) {
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
            .map(|context| context.file_rollups.len())
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
    pub(crate) fn resolve_context_git_summary(&self) -> String {
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
}
