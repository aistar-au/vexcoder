use clap::{Parser, Subcommand};
use clap_complete::Shell;

// PathBuf is not required here; subcommand variants use only String and Shell.

#[derive(Parser)]
#[command(name = "vex", about = "vexcoder -- zero-licensing-cost coding agent")]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) command: Option<Commands>,

    /// Grants `AutoApproveScope::Task` at startup, bypassing the per-tool
    /// confirmation gate for the lifetime of the process.
    #[arg(short = 'f', long = "force-unstable-alignment")]
    pub(super) force_unstable_alignment: bool,

    /// Executes a single batch turn with PROMPT text and terminates.
    /// When stdin is not a TTY, stdin content is prepended to PROMPT before
    /// dispatch. Example: `vex -p "summarise this file" < README.md`.
    #[arg(short = 'p', long = "project-map-only", value_name = "PROMPT")]
    pub(super) project_map_only: Option<String>,

    /// Sets `expand_context = true` in the resolved `Config`, raising the
    /// file-scan cardinality bound applied during context assembly.
    #[arg(short = 'e', long = "expand-sector-view")]
    pub(super) expand_sector_view: bool,

    /// Loads a persisted `TaskState` before starting the session. When
    /// TASK_ID is absent, the most-recently-modified state file in the
    /// search path is selected. Example: `vex -r task-1234` or `vex -r`.
    #[arg(short = 'r', long = "recall-coordinates", value_name = "TASK_ID",
          num_args(0..=1), default_missing_value = "")]
    pub(super) recall_coordinates: Option<String>,

    /// Sets `bypass_policy = true`, disabling durable-state disk-policy
    /// enforcement for the current process.
    #[arg(short = 'b', long = "bypass-integrity-locks")]
    pub(super) bypass_integrity_locks: bool,

    /// Resolves `ToolPolicy::Plan`, restricting the active tool set to
    /// read-only operations and enabling change-preview output. Mutually
    /// exclusive with `-t/--restrict-payload-tools`.
    #[arg(
        short = 'v',
        long = "view-intended-trajectory",
        conflicts_with = "restrict_payload_tools"
    )]
    pub(super) view_intended_trajectory: bool,

    /// Overrides `Config::model_name` with MODEL for this invocation.
    #[arg(short = 'n', long = "use-alternate-navigator", value_name = "MODEL")]
    pub(super) use_alternate_navigator: Option<String>,

    /// Configures the logging layer to emit `tracing` events at `DEBUG`
    /// severity or above. Equivalent to setting `RUST_LOG=debug`.
    #[arg(short = 'd', long = "display-internal-telemetry")]
    pub(super) display_internal_telemetry: bool,

    /// Serializes the log stream as newline-delimited JSON. Requires
    /// `-d/--display-internal-telemetry` to produce output.
    #[arg(long = "telemetry-json")]
    pub(super) telemetry_json: bool,

    /// Resolves `ToolPolicy::Plan`, restricting the active tool set to the
    /// safe read-and-search subset. Mutually exclusive with
    /// `-v/--view-intended-trajectory`.
    #[arg(
        short = 't',
        long = "restrict-payload-tools",
        conflicts_with = "view_intended_trajectory"
    )]
    pub(super) restrict_payload_tools: bool,

    /// Selects the output encoding for batch and subcommand output.
    /// `"jsonl"` emits newline-delimited JSON turn records; `"text"` (the
    /// default) emits plain text or Markdown.
    #[arg(short = 'm', long = "set-map-encoding", value_name = "FORMAT",
        value_parser = ["jsonl", "text"], default_value = "text")]
    pub(super) set_map_encoding: String,
}

#[derive(Subcommand)]
pub(super) enum Commands {
    /// Execute a non-interactive single-turn batch task.
    ///
    /// Task content is supplied via `-p/--project-map-only`. Output format is
    /// controlled by `-m/--set-map-encoding`. Auto-approval scope is derived
    /// from `-f/--force-unstable-alignment`.
    Exec,
    /// Write shell completion scripts for the target shell to stdout.
    Completions {
        /// Target shell runtime.
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Install the `prepare-commit-msg` git hook into the current repository.
    InstallHooks,
    /// Remove the `prepare-commit-msg` git hook from the current repository.
    UninstallHooks,
    /// Manage the workstation-scoped skill registry.
    Skills {
        #[command(subcommand)]
        sub: SkillsCommands,
    },
    /// Enumerate persisted parent tasks and their nested session tasks.
    ///
    /// Output format is controlled by `-m/--set-map-encoding`.
    Tasks {
        #[command(subcommand)]
        sub: TaskCommands,
    },
    /// Scaffold a new vex workspace (`.vex/config.toml`, `AGENTS.md`,
    /// `.vex/validate.toml`) in the current working directory.
    ///
    /// Non-destructive: files that already exist are skipped without error.
    Init,
    /// Create a new git branch from `HEAD` and record its name on the most
    /// recent saved task.
    Branch {
        /// Target branch name.
        name: String,
    },
    /// Propose a pull request title and body for the current branch.
    PrSummary,
    /// Run a read-only environment health check.
    ///
    /// Output format is controlled by `-m/--set-map-encoding`. Exit code is
    /// non-zero when one or more checks report failure.
    Doctor,
    /// Print the current privacy summary for the CLI and LocalApiServer
    /// surfaces.
    Privacy,
    /// Export a saved task to JSONL or Markdown.
    ///
    /// Format is controlled by `-m/--set-map-encoding`: `"jsonl"` maps to
    /// JSONL batch-turn records; `"text"` maps to Markdown. Output is always
    /// written to stdout.
    Export {
        /// Task identifier to export.
        task_id: String,
    },
    /// Start the same-machine HTTP API transport adapter.
    ///
    /// Bind address and port are read from `Config`; no per-invocation
    /// overrides are accepted on the normalized CLI surface.
    Serve,
    /// Manage OS-native credential store entries (ADR-024 Gap 38).
    ///
    /// Credentials are stored in the platform keyring under the service name
    /// `"vexcoder"`. Set `VEX_KEYRING_DISABLED=1` to bypass the keyring and
    /// fall back to `VEX_MODEL_TOKEN` for token lookup.
    Credentials {
        #[command(subcommand)]
        sub: CredentialsCommands,
    },
}

#[derive(Subcommand)]
pub(super) enum CredentialsCommands {
    /// Store a credential in the OS keyring.
    ///
    /// When stdin is not a TTY (pipe or redirect), the secret is read from
    /// stdin automatically. On an interactive TTY, a masked confirmation
    /// prompt is presented. The secret is never accepted as a positional
    /// argument, preventing leakage into shell history and process lists.
    Set {
        /// Account identifier (e.g., `model-token`).
        account: String,
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
    /// List known credential account identifiers for the `"vexcoder"` service.
    List,
}

#[derive(Subcommand)]
pub(super) enum SkillsCommands {
    /// List installed skills and their installation sources.
    List,
    /// Remove an installed skill by name.
    Remove {
        /// Registry name of the skill to remove.
        name: String,
    },
}

#[derive(Subcommand)]
pub(super) enum TaskCommands {
    /// List persisted tasks and their nested session tasks.
    ///
    /// Output format is controlled by `-m/--set-map-encoding`.
    List,
    /// Show the persisted state for one task or session-task snapshot.
    ///
    /// Output format is controlled by `-m/--set-map-encoding`.
    Watch {
        /// Task or session-task identifier.
        id: String,
    },
    /// Write the task-graph projection to
    /// `.vex/state/projections/task-graph.json` and print the file path.
    /// Creates or replaces the file atomically.
    ExportGraph,
    /// Write the todos projection to `.vex/state/projections/todos.json`
    /// and print the file path. Creates or replaces the file atomically.
    ExportTodos,
}
