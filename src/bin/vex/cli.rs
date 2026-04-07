use clap::{Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "vex", about = "vexcoder -- zero-licensing-cost coding agent")]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) command: Option<Commands>,

    /// Resume a saved task by ID, or omit the value for an interactive
    /// selection of the most-recent saved task.
    /// Example: `vex --resume task-1234` or just `vex --resume`.
    #[arg(
        long,
        num_args(0..=1),
        default_missing_value = ""
    )]
    pub(super) resume: Option<String>,

    /// Run a single prompt turn non-interactively and print the result to
    /// stdout.  Reads additional content from stdin when stdin is not a TTY.
    /// Example: `vex -p "summarise this file" < README.md`
    #[arg(short = 'p', long = "print")]
    pub(super) print_prompt: Option<String>,

    /// Use the chat/completions API format instead of the default messages/v1 format.
    /// Required when connecting to endpoints that use the chat/completions schema
    /// instead of the messages/v1 schema.
    #[arg(long = "chat-compat")]
    pub(super) chat_compat: bool,

    /// Restrict tools to read-only operations (search, read, list, git read
    /// ops, codebase_search, MCP). Mutating and shell tools are excluded.
    #[arg(long, conflicts_with = "chat")]
    pub(super) plan: bool,

    /// Disable all tool use. The model operates in plain conversation mode
    /// without access to any file, search, or shell tools.
    #[arg(long, conflicts_with = "plan")]
    pub(super) chat: bool,
}

#[derive(Subcommand)]
pub(super) enum Commands {
    /// Run a non-interactive batch task.
    Exec {
        #[arg(long, conflicts_with = "task_file")]
        task: Option<String>,
        #[arg(long = "task-file", conflicts_with = "task")]
        task_file: Option<String>,
        #[arg(long)]
        max_turns: Option<usize>,
        #[arg(long = "auto-approve", value_parser = ["once", "task"])]
        auto_approve: Option<String>,
        #[arg(long)]
        output: Option<String>,
        #[arg(long, default_value = "jsonl", value_parser = ["jsonl", "text"])]
        format: String,
    },
    /// Generate shell completion scripts and write them to stdout.
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Install the vexcoder `prepare-commit-msg` hook.
    InstallHooks,
    /// Remove the vexcoder `prepare-commit-msg` hook.
    UninstallHooks,
    /// Manage the local skills registry.
    Skills {
        #[command(subcommand)]
        sub: SkillsCommands,
    },
    /// Inspect persisted parent tasks and session tasks.
    Tasks {
        #[command(subcommand)]
        sub: TaskCommands,
    },
    /// Scaffold a new vex workspace (`.vex/config.toml`, `AGENTS.md`,
    /// `.vex/validate.toml`).  Non-destructive: skips files that already exist.
    Init {
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Create a new git branch from HEAD and record it on the most recent task.
    Branch { name: String },
    /// Generate a pull request title and body draft for the current branch.
    PrSummary,
    /// Configuration migration utilities.
    Migrate {
        #[command(subcommand)]
        sub: MigrateCommands,
    },
    /// Check environment health without starting the agent loop.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Export a saved task to JSONL or Markdown.
    Export {
        task_id: String,
        #[arg(long, default_value = "jsonl", value_parser = ["jsonl", "markdown"])]
        format: String,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        force: bool,
    },
    /// Run the local API transport adapter.
    Serve {
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
    },
}

#[derive(Subcommand)]
pub(super) enum SkillsCommands {
    /// List installed skills.
    List,
    /// Install a skill from a git repository URL or tarball URL.
    Install {
        /// Git repository URL or tarball URL.
        source: String,
        /// Select a subdirectory within the fetched source as the skill root.
        #[arg(long)]
        subdir: Option<String>,
    },
    /// Remove an installed skill by name.
    Remove {
        /// Name of the skill to remove.
        name: String,
    },
}

#[derive(Subcommand)]
pub(super) enum TaskCommands {
    /// List persisted tasks and nested session tasks.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show one persisted task or session-task snapshot.
    Watch {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Write the task-graph projection to `.vex/state/projections/task-graph.json`
    /// and print the file path.  Creates or replaces the file atomically.
    ExportGraph,
    /// Write the todos projection to `.vex/state/projections/todos.json`
    /// and print the file path.  Creates or replaces the file atomically.
    ExportTodos,
}

#[derive(Subcommand)]
pub(super) enum MigrateCommands {
    /// Map pre-ADR-022 VEX_* env var values to current config.toml keys.
    /// Reads from the environment and writes a fragment to stdout by default.
    /// Non-destructive unless `--output <path>` is passed explicitly.
    Config {
        #[arg(long)]
        output: Option<PathBuf>,
    },
}
