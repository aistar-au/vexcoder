use super::*;

pub fn build_runtime(config: Config) -> Result<(Runtime<TuiMode>, RuntimeContext)> {
    let (instructions_text, instructions_path) = match load_project_instructions(
        &config.working_dir,
        config.max_project_instructions_tokens,
    ) {
        LoadResult::Loaded(project_instructions) => {
            let display = project_instructions.path.to_string_lossy().into_owned();
            (Some(project_instructions.content), Some(display))
        }
        LoadResult::OverBudget {
            path,
            estimated_tokens,
        } => {
            eprintln!(
                "[project instructions] {} skipped: estimated {} tokens exceeds budget of {}",
                path.display(),
                estimated_tokens,
                config.max_project_instructions_tokens,
            );
            (None, None)
        }
        LoadResult::NotFound => (None, None),
    };

    let (client, notes_warning) = build_api_client_with_notes(&config)?;
    let client = client.with_project_instructions(instructions_text);
    let operator = ToolOperator::new(config.working_dir.clone());
    let conversation = ConversationManager::new_with_hooks(client, operator, config.hooks.clone());

    let (update_tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());

    let mut mode = TuiMode::new_with_config(config.notes_path.clone(), config);
    mode.instructions_path = instructions_path;
    mode.current_task.instructions_path = mode.instructions_path.clone();
    if let Some(warning) = notes_warning {
        mode.push_history_line(warning);
    }
    let runtime = Runtime::new(mode, update_rx);
    Ok((runtime, ctx))
}

/// Build a runtime and immediately apply a pre-loaded resume state.
/// Called from `src/bin/vex.rs` when `--resume` is passed at startup.
pub fn build_runtime_with_resume(
    config: Config,
    resume_state: TaskState,
) -> Result<(Runtime<TuiMode>, RuntimeContext)> {
    let (mut runtime, ctx) = build_runtime(config)?;
    let restored_id = resume_state.id.clone();
    let status = format!("{:?}", resume_state.status);
    runtime.mode.current_task = resume_state;
    if let Some(path) = runtime.mode.current_task.instructions_path.clone() {
        runtime.mode.instructions_path = Some(path);
    } else {
        runtime.mode.current_task.instructions_path = runtime.mode.instructions_path.clone();
    }
    runtime
        .mode
        .push_history_line(format!("[resumed: {restored_id} status={status}]"));
    Ok((runtime, ctx))
}
