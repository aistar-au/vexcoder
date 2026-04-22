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
    
    
    pub next_step_id: u64,
}


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


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantBlockEntry {
    
    pub block_index: usize,
    pub phase: AssistantPhase,
    pub content: String,
    pub collapsed: bool,
    pub streaming: bool,
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantPhase {
    Thinking,
    Final,
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalDocument {
    pub step_id: u64,
    pub capability: Capability,
    pub scope: ApprovalScope,
    pub tool_name: Option<String>,
    pub input_preview: String,
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandSessionDocument {
    pub session_id: u64,
    pub command: String,
    pub pid: Option<u32>,
    pub status: String,
    pub output_tail: Vec<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TurnOutcome {
    Completed,
    Failed { message: String },
    Cancelled,
    MaxTurnsReached,
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoticeSeverity {
    Info,
    Warning,
    Error,
}


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
