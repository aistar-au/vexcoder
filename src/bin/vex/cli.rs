use clap::{Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "vex", about = "vexcoder -- zero-licensing-cost coding agent")]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) command: Option<Commands>,

    /// Automatically approve tool requests for the current session or batch task.
    #[arg(short = 'f', long = "force-unstable-alignment")]
    pub(super) force_unstable_alignment: bool,

    /// Non-interactive: run one prompt turn and print result to stdout.
    /// Reads additional content from stdin when stdin is not a TTY.
    /// Example: `vex -p "summarise this file" < README.md`
    #[arg(short = 'p', long = "project-map-only")]
    pub(super) project_map_only: Option<String>,

    /// Expands inferred related-path and directory scan limits for context assembly.
    #[arg(short = 'e', long = "expand-sector-view")]
    pub(super) expand_sector_view: bool,

    /// Resume a saved task by ID, or omit the value for the most-recent saved task.
    /// Example: `vex -r task-1234` or just `vex -r`.
    #[arg(short = 'r', long = "recall-coordinates", num_args(0..=1), default_missing_value = "")]
    pub(super) recall_coordinates: Option<String>,

    /// Disables durable-state disk-policy enforcement for the current process.
    #[arg(short = 'b', long = "bypass-integrity-locks")]
    pub(super) bypass_integrity_locks: bool,

    /// Select the existing read-only planning tool policy.
    #[arg(
        short = 'v',
        long = "view-intended-trajectory",
        conflicts_with = "restrict_payload_tools"
    )]
    pub(super) view_intended_trajectory: bool,

    /// Override the configured model identifier.
    #[arg(short = 'n', long = "use-alternate-navigator", value_name = "MODEL")]
    pub(super) use_alternate_navigator: Option<String>,

    /// Emit internal transport and normalization telemetry to stderr.
    #[arg(short = 'd', long = "display-internal-telemetry")]
    pub(super) display_internal_telemetry: bool,

    /// Format internal telemetry as newline-delimited JSON.
    #[arg(long = "telemetry-json")]
    pub(super) telemetry_json: bool,

    /// Select the existing safe read/search tool subset directly.
    #[arg(
        short = 't',
        long = "restrict-payload-tools",
        conflicts_with = "view_intended_trajectory"
    )]
    pub(super) restrict_payload_tools: bool,

    /// Set output encoding: "jsonl" | "text" (default: text).
    #[arg(short = 'm', long = "set-map-encoding", value_name = "FORMAT",
        value_parser = ["jsonl", "text"], default_value = "text")]
    pub(super) set_map_encoding: String,
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
    /// Manage the workstation-scoped skills registry.
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
    /// Generate a proposed pull request title and body for the current branch.
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
    /// Print the current privacy summary for the CLI and LocalApiServer surfaces.
    Privacy,
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
    /// Run the same-machine API transport adapter.
    Serve {
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
    },
    /// Manage OS-native credential store entries (ADR-024 Gap 38).
    ///
    /// Credentials are stored in the platform keyring under the service name
    /// "vexcoder".
    /// Set VEX_KEYRING_DISABLED=1 to bypass the keyring and use only
    /// VEX_MODEL_TOKEN for token lookup.
    Credentials {
        #[command(subcommand)]
        sub: CredentialsCommands,
    },
}

#[derive(Subcommand)]
pub(super) enum CredentialsCommands {
    /// Store a credential in the OS keyring.
    ///
    /// Example:
    /// `printf '%s' "$VEX_MODEL_TOKEN" | vex credentials set model-token --stdin`
    /// If no option is passed on an interactive TTY, vex prompts for the secret
    /// without echoing it.
    Set {
        /// Account identifier (e.g., `model-token`).
        account: String,
        /// Read the secret from piped or redirected stdin instead of argv.
        #[arg(long, conflicts_with = "from_env")]
        stdin: bool,
        /// Read the secret from the named environment variable.
        #[arg(long = "from-env", value_name = "VAR", conflicts_with = "stdin")]
        from_env: Option<String>,
    },
    /// Read a credential from the OS keyring and print it to stdout.
    Get {
        /// Account identifier (e.g., `model-token`).
        account: String,
    },
    /// Delete a credential from the OS keyring.
    Delete {
        /// Account identifier (e.g., `model-token`).
        account: String,
    },
    /// List known credential account identifiers for this service.
    List,
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
