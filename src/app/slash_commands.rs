#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SlashCommandId {
    Quit,
    Exit,
    About,
    MemoryShow,
    MemoryAdd,
    MemoryClear,
    MemoryAutoOn,
    MemoryAutoOff,
    MemoryAutoClear,
    New,
    Resume,
    Compact,
    Fork,
    Permissions,
    Allow,
    Deny,
    Model,
    Diff,
    Edit,
    Fix,
    Explain,
    Review,
    Plan,
    Init,
    Run,
    Test,
    Context,
    Mcp,
    Tools,
    Usage,
    GenerateTests,
    Agents,
    Delegate,
    Watch,
    Undo,
    Commands,
    Help,
    Reindex,
    Copy,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum SlashCommandPattern {
    Exact(&'static str),
    ExactOrPrefix {
        exact: &'static str,
        prefix: &'static str,
    },
}

impl SlashCommandPattern {
    pub(super) fn parse<'a>(&self, input: &'a str) -> Option<&'a str> {
        match self {
            SlashCommandPattern::Exact(command) => (input == *command).then_some(""),
            SlashCommandPattern::ExactOrPrefix { exact, prefix } => {
                if input == *exact {
                    Some("")
                } else {
                    input.strip_prefix(prefix).map(str::trim)
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SlashCommandSpec {
    pub(super) id: SlashCommandId,
    pub(super) pattern: SlashCommandPattern,
    pub(super) display: &'static str,
    pub(super) description: &'static str,
}

impl SlashCommandSpec {
    const fn new(
        id: SlashCommandId,
        pattern: SlashCommandPattern,
        display: &'static str,
        description: &'static str,
    ) -> Self {
        assert!(
            !display.is_empty(),
            "slash command display must not be empty"
        );
        assert!(
            !description.is_empty(),
            "slash command description must not be empty"
        );
        Self {
            id,
            pattern,
            display,
            description,
        }
    }
}

pub(super) const SLASH_COMMANDS: &[SlashCommandSpec] = &[
    SlashCommandSpec::new(
        SlashCommandId::Edit,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/edit",
            prefix: "/edit ",
        },
        "/edit <instruction>",
        "start an edit loop",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Fix,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/fix",
            prefix: "/fix ",
        },
        "/fix",
        "re-run edit loop from last error",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Explain,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/explain",
            prefix: "/explain ",
        },
        "/explain [path]",
        "explain a file or region; no patch",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Review,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/review",
            prefix: "/review ",
        },
        "/review [--base <git-ref>] [--files <glob>] [<instruction>]",
        "review a diff or file set; no patch",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Plan,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/plan",
            prefix: "/plan ",
        },
        "/plan <instruction>",
        "generate an implementation plan; no patch",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Init,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/init",
            prefix: "/init ",
        },
        "/init [environment]",
        "scaffold .vex files for the current workspace",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Run,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/run",
            prefix: "/run ",
        },
        "/run [command]",
        "run a command; no model turn",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Test,
        SlashCommandPattern::Exact("/test"),
        "/test",
        "run full validation suite; no model turn",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Context,
        SlashCommandPattern::Exact("/context"),
        "/context",
        "show session context summary; no model turn",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Mcp,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/mcp",
            prefix: "/mcp ",
        },
        "/mcp [list|show <server>]",
        "show loaded MCP servers and tools; no model turn",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Tools,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/tools",
            prefix: "/tools ",
        },
        "/tools [desc]",
        "show active tool registry; no model turn",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Usage,
        SlashCommandPattern::Exact("/usage"),
        "/usage",
        "show session token usage; no model turn",
    ),
    SlashCommandSpec::new(
        SlashCommandId::GenerateTests,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/generate-tests",
            prefix: "/generate-tests ",
        },
        "/generate-tests [path] [--framework <name>]",
        "generate tests for a path; test files only",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Model,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/model",
            prefix: "/model ",
        },
        "/model [<n>]",
        "show or switch active model name",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Permissions,
        SlashCommandPattern::Exact("/permissions"),
        "/permissions",
        "show active capability grants",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Allow,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/allow",
            prefix: "/allow ",
        },
        "/allow <cap> [once|session]",
        "grant a capability",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Deny,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/deny",
            prefix: "/deny ",
        },
        "/deny <cap>",
        "revoke a capability",
    ),
    SlashCommandSpec::new(
        SlashCommandId::New,
        SlashCommandPattern::Exact("/new"),
        "/new",
        "save and reset session",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Resume,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/resume",
            prefix: "/resume ",
        },
        "/resume [<task-id>]",
        "resume a saved session",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Compact,
        SlashCommandPattern::Exact("/compact"),
        "/compact",
        "compact conversation history (keep task)",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Fork,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/fork",
            prefix: "/fork ",
        },
        "/fork [<label>]",
        "fork current session to new task-id",
    ),
    SlashCommandSpec::new(
        SlashCommandId::MemoryShow,
        SlashCommandPattern::Exact("/memory"),
        "/memory [add <note>|clear]",
        "view or edit persistent user notes",
    ),
    SlashCommandSpec::new(
        SlashCommandId::MemoryAdd,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/memory add",
            prefix: "/memory add ",
        },
        "/memory [add <note>|clear]",
        "view or edit persistent user notes",
    ),
    SlashCommandSpec::new(
        SlashCommandId::MemoryClear,
        SlashCommandPattern::Exact("/memory clear"),
        "/memory [add <note>|clear]",
        "view or edit persistent user notes",
    ),
    SlashCommandSpec::new(
        SlashCommandId::MemoryAutoOn,
        SlashCommandPattern::Exact("/memory auto on"),
        "/memory auto [on|off|clear]",
        "toggle or clear automatic note extraction",
    ),
    SlashCommandSpec::new(
        SlashCommandId::MemoryAutoOff,
        SlashCommandPattern::Exact("/memory auto off"),
        "/memory auto [on|off|clear]",
        "toggle or clear automatic note extraction",
    ),
    SlashCommandSpec::new(
        SlashCommandId::MemoryAutoClear,
        SlashCommandPattern::Exact("/memory auto clear"),
        "/memory auto [on|off|clear]",
        "toggle or clear automatic note extraction",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Agents,
        SlashCommandPattern::Exact("/agents"),
        "/agents",
        "show configured agents and teams",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Delegate,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/delegate",
            prefix: "/delegate ",
        },
        "/delegate <agent> <prompt>",
        "create a session task for a configured agent",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Watch,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/watch",
            prefix: "/watch ",
        },
        "/watch [task-id|agent-id]",
        "show session-task status for the current repo",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Undo,
        SlashCommandPattern::Exact("/undo"),
        "/undo",
        "undo last file-modifying tool call",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Commands,
        SlashCommandPattern::Exact("/commands"),
        "/commands",
        "show this list",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Help,
        SlashCommandPattern::Exact("/help"),
        "/help",
        "alias for /commands",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Reindex,
        SlashCommandPattern::Exact("/reindex"),
        "/reindex",
        "force a full rebuild of the codebase search index",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Quit,
        SlashCommandPattern::Exact("/quit"),
        "/quit",
        "save and exit",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Exit,
        SlashCommandPattern::Exact("/exit"),
        "/exit",
        "alias for /quit",
    ),
    SlashCommandSpec::new(
        SlashCommandId::About,
        SlashCommandPattern::Exact("/about"),
        "/about",
        "show version and build info",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Copy,
        SlashCommandPattern::Exact("/copy"),
        "/copy",
        "copy last assistant output to clipboard",
    ),
    SlashCommandSpec::new(
        SlashCommandId::Diff,
        SlashCommandPattern::ExactOrPrefix {
            exact: "/diff",
            prefix: "/diff ",
        },
        "/diff [--staged]",
        "show working-tree diff; no model turn",
    ),
];

pub(super) fn slash_command_menu_group(id: SlashCommandId) -> &'static str {
    match id {
        SlashCommandId::Plan
        | SlashCommandId::Explain
        | SlashCommandId::Review
        | SlashCommandId::Context
        | SlashCommandId::Mcp
        | SlashCommandId::Tools
        | SlashCommandId::GenerateTests
        | SlashCommandId::Agents
        | SlashCommandId::Watch
        | SlashCommandId::Reindex => "retrieve + context",
        SlashCommandId::Edit | SlashCommandId::Fix | SlashCommandId::Diff => "edit + inspect",
        SlashCommandId::Run | SlashCommandId::Test | SlashCommandId::Delegate => {
            "validate + execute"
        }
        SlashCommandId::Init
        | SlashCommandId::Model
        | SlashCommandId::Permissions
        | SlashCommandId::Allow
        | SlashCommandId::Deny
        | SlashCommandId::Usage
        | SlashCommandId::Commands
        | SlashCommandId::Help
        | SlashCommandId::MemoryShow
        | SlashCommandId::MemoryAdd
        | SlashCommandId::MemoryClear
        | SlashCommandId::MemoryAutoOn
        | SlashCommandId::MemoryAutoOff
        | SlashCommandId::MemoryAutoClear
        | SlashCommandId::New
        | SlashCommandId::Resume
        | SlashCommandId::Compact
        | SlashCommandId::Fork
        | SlashCommandId::Quit
        | SlashCommandId::Exit
        | SlashCommandId::About
        | SlashCommandId::Copy => "session + control",
        SlashCommandId::Undo => "edit + inspect",
    }
}

pub(super) fn slash_command_mode_summary(id: SlashCommandId) -> &'static str {
    match id {
        SlashCommandId::Plan => "read-only plan from current repo context",
        SlashCommandId::Explain => "read-only explanation with context assembly",
        SlashCommandId::Review => "read-only review over assembled context",
        SlashCommandId::Context => "session status, git state, and token summary",
        SlashCommandId::Mcp => "inspect loaded MCP servers and per-server tool inventory",
        SlashCommandId::Tools => "tool directory plus retrieval workflow guidance",
        SlashCommandId::GenerateTests => "assemble context and draft tests for one path",
        SlashCommandId::Agents => "show configured agents, teams, and active session-task counts",
        SlashCommandId::Watch => "inspect persisted session-task status by id or agent",
        SlashCommandId::Reindex => "force a full structural rebuild of the codebase search index",
        SlashCommandId::Edit | SlashCommandId::Fix => "edit loop that may patch files",
        SlashCommandId::Diff => "git diff preview without starting a model turn",
        SlashCommandId::Run | SlashCommandId::Test => "local validation only; no model turn",
        SlashCommandId::Delegate => "create a persisted session task for a configured agent",
        SlashCommandId::Init => "write .vex scaffolding in the current workspace",
        SlashCommandId::Model => "show or switch the active model name",
        SlashCommandId::Permissions | SlashCommandId::Allow | SlashCommandId::Deny => {
            "inspect or change capability grants"
        }
        SlashCommandId::Usage => "show last-turn and session token counts",
        SlashCommandId::Commands | SlashCommandId::Help => "show grouped operator command menu",
        SlashCommandId::MemoryShow | SlashCommandId::MemoryAdd | SlashCommandId::MemoryClear => {
            "view or update persistent notes"
        }
        SlashCommandId::MemoryAutoOn
        | SlashCommandId::MemoryAutoOff
        | SlashCommandId::MemoryAutoClear => "toggle automatic note extraction after each turn",
        SlashCommandId::New
        | SlashCommandId::Resume
        | SlashCommandId::Compact
        | SlashCommandId::Fork => "manage saved session state",
        SlashCommandId::Quit | SlashCommandId::Exit => "save state and exit",
        SlashCommandId::About => "show build and environment info",
        SlashCommandId::Copy => "copy last assistant output to the system clipboard",
        SlashCommandId::Undo => "revert last file-modifying tool call from checkpoint stack",
    }
}

pub(super) fn builtin_tool_menu_group(name: &str) -> &'static str {
    match name {
        "list_files" | "list_directory" | "list_dir" | "glob_files" | "find_files" | "search"
        | "search_files" | "search_content" | "codebase_search" | "read_file" => "retrieve",
        "write_file" | "edit_file" | "apply_patch" | "rename_file" => "mutate",
        "git_status" | "git_diff" | "git_log" | "git_show" | "git_add" | "git_commit" => "git",
        _ => "other",
    }
}

pub(super) fn builtin_tool_usage_hint(name: &str) -> &'static str {
    match name {
        "list_files" | "list_directory" | "list_dir" => {
            "start broad at the workspace or directory level"
        }
        "glob_files" => "find files by name pattern across the workspace",
        "find_files" => "narrow to filename matches before reading content",
        "search" | "search_content" => "scan exact text or regex hits across files",
        "codebase_search" => "rank functions, types, and code snippets before opening files",
        "read_file" => "read exact paths after discovery narrows the target",
        "write_file" | "edit_file" | "apply_patch" | "rename_file" => {
            "make targeted workspace mutations"
        }
        "git_status" | "git_diff" | "git_log" | "git_show" | "git_add" | "git_commit" => {
            "inspect and record repository state"
        }
        _ => "built-in tool",
    }
}
