use super::*;
use crate::api::ApiClient;
use crate::mcp::{McpRegistry, McpRegistryRollup};
use crate::runtime::frontend::FrontendAdapter;
use crate::runtime::{resolve_configured_sandbox, ConfiguredSandbox};

#[derive(Clone, Debug, Default)]
pub struct FacadeBootstrap {
    pub instructions_path: Option<String>,
    pub notes_warning: Option<String>,
    pub sandbox: ConfiguredSandbox,
    pub sandbox_warning: Option<String>,
    pub mcp_rollup: Option<McpRegistryRollup>,
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
            sandbox: ConfiguredSandbox::default(),
            sandbox_warning: None,
            mcp_rollup: None,
        },
    ))
}

pub fn build_facade_runtime<M: RuntimeMode>(
    config: &Config,
    mode: M,
) -> AppResult<(Runtime<M>, RuntimeContext, FacadeBootstrap)> {
    let (sandbox, sandbox_warning) = resolve_configured_sandbox(&config.sandbox)?;
    crate::state::warm_codebase_index_with_config(&config.working_dir, &config.search);
    let mcp_registry = McpRegistry::connect_all_blocking(&config.mcp_servers)?;
    let mcp_rollup = mcp_registry.as_ref().map(|registry| registry.rollup());
    let (client, mut bootstrap) = build_facade_client(config)?;
    let client = client.with_extra_tool_definitions(
        mcp_registry
            .as_ref()
            .map(|registry| registry.tool_definitions())
            .unwrap_or_default(),
    );
    bootstrap.sandbox = sandbox.clone();
    bootstrap.sandbox_warning = sandbox_warning;
    bootstrap.mcp_rollup = mcp_rollup;
    let operator = ToolOperator::new(config.working_dir.clone());
    let conversation = ConversationManager::new_with_hooks_full(
        client,
        operator,
        config.hooks.clone(),
        config.http_hooks.clone(),
    )
    .with_search_config(config.search.clone())
    .with_sandbox(sandbox)
    .with_mcp_registry(mcp_registry)
    .with_undo_enabled(config.undo.enabled)
    .with_max_undo_checkpoints(config.undo.max_checkpoints);
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
    ctx.shutdown_resources().await;
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
    ctx.populate_local_server_info().await;
    runtime.run(frontend, &mut ctx).await;
    ctx.shutdown_resources().await;
    Ok(())
}
