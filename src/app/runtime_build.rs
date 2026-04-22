use super::*;

pub fn build_runtime(config: Config) -> Result<(Runtime<TuiMode>, RuntimeContext)> {
    let mode = TuiMode::new_with_config(config.notes_path.clone(), config.clone());
    let (mut runtime, ctx, bootstrap) = build_facade_runtime(&config, mode)?;
    runtime.mode.instructions_path = bootstrap.instructions_path;
    runtime.mode.sandbox = bootstrap.sandbox;
    runtime.mode.mcp_rollup = bootstrap.mcp_rollup;
    runtime.mode.task_doc.info.instructions_path = runtime.mode.instructions_path.clone();
    if let Some(warning) = bootstrap.notes_warning {
        runtime.mode.push_history_line(warning);
    }
    if let Some(warning) = bootstrap.sandbox_warning {
        runtime.mode.push_history_line(warning);
    }
    Ok((runtime, ctx))
}


pub fn build_runtime_with_resume(
    config: Config,
    resume_state: TaskState,
) -> Result<(Runtime<TuiMode>, RuntimeContext)> {
    let (mut runtime, ctx) = build_runtime(config)?;
    let restored_id = resume_state.id.clone();
    let status = format!("{:?}", resume_state.status);
    let task_doc = runtime
        .mode
        .task_doc_condenser
        .restore_from_snapshot(resume_state);
    runtime.mode.task_doc = task_doc;
    if let Some(path_str) = runtime.mode.task_doc.info.instructions_path.clone() {
        runtime.mode.instructions_path = Some(path_str);
    } else {
        runtime.mode.task_doc.info.instructions_path = runtime.mode.instructions_path.clone();
    }
    runtime
        .mode
        .push_history_line(format!("[resumed: {restored_id} status={status}]"));
    Ok((runtime, ctx))
}
