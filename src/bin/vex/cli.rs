use clap::{Parser, Subcommand};
use clap_complete::Shell;

#[derive(Parser)]
#[command(name = "vex", about = "vexcoder -- zero-licensing-cost coding agent")]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) command: Option<Commands>,

    #[arg(short = 'f', long = "force-unstable-alignment")]
    pub(super) force_unstable_alignment: bool,

    #[arg(short = 'p', long = "project-map-only", value_name = "PROMPT")]
    pub(super) project_map_only: Option<String>,

    #[arg(short = 'e', long = "expand-sector-view")]
    pub(super) expand_sector_view: bool,

    #[arg(short = 'r', long = "recall-coordinates", value_name = "TASK_ID",
          num_args(0..=1), default_missing_value = "")]
    pub(super) recall_coordinates: Option<String>,

    #[arg(short = 'b', long = "bypass-integrity-locks")]
    pub(super) bypass_integrity_locks: bool,

    #[arg(
        short = 'v',
        long = "view-intended-trajectory",
        conflicts_with = "restrict_payload_tools"
    )]
    pub(super) view_intended_trajectory: bool,

    #[arg(short = 'n', long = "use-alternate-navigator", value_name = "MODEL")]
    pub(super) use_alternate_navigator: Option<String>,

    #[arg(short = 'd', long = "display-internal-telemetry")]
    pub(super) display_internal_telemetry: bool,

    #[arg(long = "telemetry-json")]
    pub(super) telemetry_json: bool,

    #[arg(
        short = 't',
        long = "restrict-payload-tools",
        conflicts_with = "view_intended_trajectory"
    )]
    pub(super) restrict_payload_tools: bool,

    #[arg(short = 'm', long = "set-map-encoding", value_name = "FORMAT",
        value_parser = ["jsonl", "text"], default_value = "text")]
    pub(super) set_map_encoding: String,
}

#[derive(Subcommand)]
pub(super) enum Commands {
    Exec,

    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },

    InstallHooks,

    UninstallHooks,

    Skills {
        #[command(subcommand)]
        sub: SkillsCommands,
    },

    Tasks {
        #[command(subcommand)]
        sub: TaskCommands,
    },

    Init,

    Branch {
        name: String,
    },

    PrSummary,

    Doctor,

    Privacy,

    Export {
        task_id: String,
    },

    Serve,

    Credentials {
        #[command(subcommand)]
        sub: CredentialsCommands,
    },
}

#[derive(Subcommand)]
pub(super) enum CredentialsCommands {
    Set { account: String },

    Get { account: String },

    Delete { account: String },

    List,
}

#[derive(Subcommand)]
pub(super) enum SkillsCommands {
    List,

    Remove { name: String },
}

#[derive(Subcommand)]
pub(super) enum TaskCommands {
    List,

    Watch { id: String },

    ExportGraph,

    ExportTodos,
}
