use super::*;
use crate::api::ApiClient;
use crate::runtime::frontend::FrontendAdapter;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FacadeBootstrap {
    pub instructions_path: Option<String>,
    pub notes_warning: Option<String>,
}

pub fn build_facade_client(config: &Config) -> AppResult<(ApiClient, FacadeBootstrap)> {
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

    let (client, notes_warning) = build_api_client_with_notes(config)?;
    Ok((
        client.with_project_instructions(instructions_text),
        FacadeBootstrap {
            instructions_path,
            notes_warning,
        },
    ))
}

pub fn build_facade_runtime<M: RuntimeMode>(
    config: &Config,
    mode: M,
) -> AppResult<(Runtime<M>, RuntimeContext, FacadeBootstrap)> {
    let (client, bootstrap) = build_facade_client(config)?;
    let operator = ToolOperator::new(config.working_dir.clone());
    let conversation = ConversationManager::new_with_hooks(client, operator, config.hooks.clone());
    let (update_tx, update_rx) = mpsc::unbounded_channel::<UiUpdate>();
    let ctx = RuntimeContext::new(conversation, update_tx, CancellationToken::new());
    let runtime = Runtime::new(mode, update_rx);
    Ok((runtime, ctx, bootstrap))
}

pub async fn execute_facade_runtime<M, F>(
    config: &Config,
    mode: M,
    frontend: &mut F,
) -> AppResult<FacadeBootstrap>
where
    M: RuntimeMode,
    F: FrontendAdapter<M>,
{
    let (mut runtime, mut ctx, bootstrap) = build_facade_runtime(config, mode)?;
    runtime.run(frontend, &mut ctx).await;
    Ok(bootstrap)
}

pub async fn run_tui_session<F>(
    config: Config,
    resume_state: Option<TaskState>,
    frontend: &mut F,
) -> AppResult<()>
where
    F: FrontendAdapter<TuiMode>,
{
    let (mut runtime, mut ctx) = match resume_state {
        Some(state) => build_runtime_with_resume(config, state)?,
        None => build_runtime(config)?,
    };
    runtime.run(frontend, &mut ctx).await;
    Ok(())
}
