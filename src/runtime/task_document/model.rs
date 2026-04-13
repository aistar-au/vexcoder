use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use crate::runtime::session_task::SessionTask;
use crate::runtime::task_state::{
    CommandEvidence, ContextCompactionRecord, SessionNote, TaskStatus,
};
use crate::runtime::{ApprovalScope, Capability, ModelBackendKind};
use crate::state::{ToolStatus, TurnToolPolicy};
use crate::types::{StreamPromptProgress, StreamTimings};
use crate::usage::TurnTokens;

/// Canonical in-process task document. All turn data are kept here; the renderer,
/// local API, and batch mode each project from this type rather than
/// maintaining a second copy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskDocument {
    pub info: TaskInfo,
    pub completed_turns: Vec<TurnDocument>,
    pub active_turn: Option<ActiveTurnDocument>,
    pub session_notes: Vec<SessionNote>,
    pub context_compaction: Vec<ContextCompactionRecord>,
    pub session_tasks: Vec<SessionTask>,
    pub last_error: Option<TaskErrorState>,
}

/// Stable task-level metadata that does not change turn-to-turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskInfo {
    pub id: String,
    pub status: TaskStatus,
    pub parent_task_id: Option<String>,
    pub agent_id: Option<String>,
    pub worktree_path: Option<PathBuf>,
    pub branch_name: Option<String>,
    pub instructions_path: Option<String>,
    pub model_name: String,
    pub model_backend: ModelBackendKind,
    pub model_url: String,
    pub started_at_ms: Option<u64>,
    pub updated_at_ms: u64,
    pub last_heartbeat_ms: Option<u64>,
    pub active_grants: HashMap<Capability, ApprovalScope>,
    /// Monotonically increasing counter used to assign stable step IDs to
    /// every ordered entry in a turn. Never resets across turns.
    pub next_step_id: u64,
}

/// Live state for a turn that is currently in progress.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActiveTurnDocument {
    pub turn_index: usize,
    pub input: String,
    pub entries: Vec<TurnEntry>,
    pub started_at_ms: u64,
    pub ttft_ms: Option<u64>,
    pub prompt_progress: Option<StreamPromptProgress>,
    pub timings: Option<StreamTimings>,
    pub pending_approval: Option<ApprovalDocument>,
    pub command_sessions: BTreeMap<u64, CommandSessionDocument>,
    pub changed_files: BTreeSet<String>,
    pub command_history: Vec<CommandEvidence>,
    #[serde(skip, default)]
    pub tool_policy: TurnToolPolicy,
    pub cancel_pending: bool,
}

/// Immutable record of a completed turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TurnDocument {
    pub turn_index: usize,
    pub input: String,
    pub entries: Vec<TurnEntry>,
    pub outcome: TurnOutcome,
    pub changed_files: Vec<String>,
    pub command_history: Vec<CommandEvidence>,
    pub tokens: TurnTokens,
    pub started_at_ms: u64,
    pub completed_at_ms: u64,
    pub ttft_ms: Option<u64>,
    pub timings: Option<StreamTimings>,
}

/// Ordered, typed record of one piece of turn content. The full turn
/// transcript and timeline are both projected from this sequence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnEntry {
    UserInput {
        step_id: u64,
        text: String,
    },
    AssistantBlock {
        step_id: u64,
        block: AssistantBlockEntry,
    },
    ToolCall {
        step_id: u64,
        id: String,
        name: String,
        input: serde_json::Value,
        status: ToolStatus,
    },
    ToolResult {
        step_id: u64,
        tool_call_id: String,
        tool_name: Option<String>,
        output: String,
        is_error: bool,
    },
    ApprovalRequest {
        step_id: u64,
        approval: ApprovalDocument,
    },
    ApprovalResolved {
        step_id: u64,
        capability: Capability,
        scope: ApprovalScope,
        approved: bool,
    },
    CommandSession {
        step_id: u64,
        session: CommandSessionDocument,
    },
    SystemNotice {
        step_id: u64,
        message: String,
        severity: NoticeSeverity,
    },
}

/// One assistant text block within a turn (thinking or final text).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantBlockEntry {
    /// Index matching the `TranscriptBlockStart` index from the runtime stream.
    pub block_index: usize,
    pub phase: AssistantPhase,
    pub content: String,
    pub collapsed: bool,
    pub streaming: bool,
}

/// Which phase of assistant output this block represents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantPhase {
    Thinking,
    Final,
}

/// Approval state for a capability request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalDocument {
    pub step_id: u64,
    pub capability: Capability,
    pub scope: ApprovalScope,
    pub tool_name: Option<String>,
    pub input_preview: String,
}

/// In-process command session state tracked during a turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandSessionDocument {
    pub session_id: u64,
    pub command: String,
    pub pid: Option<u32>,
    pub status: String,
    pub output_tail: Vec<String>,
}

/// How a completed turn ended.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TurnOutcome {
    Completed,
    Failed { message: String },
    Cancelled,
    MaxTurnsReached,
}

/// Severity level for system notices appended to the turn entry stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoticeSeverity {
    Info,
    Warning,
    Error,
}

/// Non-recoverable task error state stored outside the turn list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskErrorState {
    pub message: String,
    pub recoverable: bool,
}

impl std::str::FromStr for Capability {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "read_file" | "read-file" => Ok(Capability::ReadFile),
            "write_file" | "write-file" => Ok(Capability::WriteFile),
            "apply_patch" | "apply-patch" => Ok(Capability::ApplyPatch),
            "run_command" | "run-command" => Ok(Capability::RunCommand),
            "mcp_tool" | "mcp-tool" => Ok(Capability::McpTool),
            "network" => Ok(Capability::Network),
            "browser" => Ok(Capability::Browser),
            other => Err(format!("unknown capability: {other}")),
        }
    }
}

impl std::str::FromStr for ApprovalScope {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "once" => Ok(ApprovalScope::Once),
            "task" => Ok(ApprovalScope::Task),
            "session" => Ok(ApprovalScope::Session),
            other => Err(format!("unknown scope: {other}")),
        }
    }
}
