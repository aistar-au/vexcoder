use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use crate::runtime::json_handoff::RuntimeEvent;
use crate::runtime::session_task::SessionTask;
use crate::runtime::task_state::{
    CommandEvidence, ContextCompactionRecord, SessionNote, TaskState, TaskStatus,
};
use crate::runtime::{ApprovalScope, Capability, ModelBackendKind};
use crate::state::{StreamBlock, ToolStatus, TurnToolPolicy};
use crate::types::{StreamPromptProgress, StreamTimings};
use crate::usage::TurnTokens;

/// Canonical in-process task document.  All turn data lives here; the
/// renderer, local API, and batch mode each project from this type rather
/// than maintaining a second copy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskDocument {
    pub meta: TaskMeta,
    pub completed_turns: Vec<TurnDocument>,
    pub active_turn: Option<ActiveTurnDocument>,
    pub session_notes: Vec<SessionNote>,
    pub context_compaction: Vec<ContextCompactionRecord>,
    pub session_tasks: Vec<SessionTask>,
    pub last_error: Option<TaskErrorState>,
}

/// Stable task-level metadata that does not change turn-to-turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskMeta {
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
    /// every ordered entry in a turn.  Never resets across turns.
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

/// Ordered, typed record of one piece of turn content.  The full turn
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

// ---------------------------------------------------------------------------
// Reducer
// ---------------------------------------------------------------------------

/// Stateless reducer that applies [`RuntimeEvent`] mutations to a
/// [`TaskDocument`] and produces snapshot adapters compatible with the
/// existing [`TaskState`] persistence format.
#[derive(Debug, Default)]
pub struct TaskDocumentReducer;

/// Summary of what changed after a single reducer call.  Callers use this
/// to drive incremental UI updates without re-rendering the whole document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskMutationSummary {
    pub task_status_changed: bool,
    pub active_turn_changed: bool,
    pub appended_rows: usize,
    pub selected_step_id: Option<u64>,
    pub approval_changed: bool,
}

impl TaskDocumentReducer {
    pub fn new() -> Self {
        Self
    }

    /// Construct an empty document for a newly-created task.
    pub fn begin_task(&self, meta: TaskMeta) -> TaskDocument {
        TaskDocument {
            meta,
            completed_turns: Vec::new(),
            active_turn: None,
            session_notes: Vec::new(),
            context_compaction: Vec::new(),
            session_tasks: Vec::new(),
            last_error: None,
        }
    }

    /// Open a new active turn.  Panics if `doc.active_turn` is already set
    /// because the previous turn must be finished before starting the next.
    pub fn begin_turn(
        &self,
        doc: &mut TaskDocument,
        input: String,
        started_at_ms: u64,
        tool_policy: TurnToolPolicy,
    ) {
        assert!(
            doc.active_turn.is_none(),
            "begin_turn called while an active turn is already open"
        );
        let turn_index = doc.completed_turns.len();
        let step_id = doc.meta.next_step_id;
        doc.meta.next_step_id += 1;
        doc.meta.status = TaskStatus::Running;
        doc.meta.updated_at_ms = started_at_ms;
        doc.active_turn = Some(ActiveTurnDocument {
            turn_index,
            input: input.clone(),
            entries: vec![TurnEntry::UserInput {
                step_id,
                text: input,
            }],
            started_at_ms,
            ttft_ms: None,
            prompt_progress: None,
            timings: None,
            pending_approval: None,
            command_sessions: BTreeMap::new(),
            changed_files: BTreeSet::new(),
            command_history: Vec::new(),
            tool_policy,
            cancel_pending: false,
        });
    }

    /// Apply one [`RuntimeEvent`] to the document.
    ///
    /// Events that do not affect the active turn (e.g. events produced before
    /// `begin_turn` or after `finish_turn`) are silently ignored so callers
    /// do not need to gate every call.
    pub fn apply_runtime_event(
        &self,
        doc: &mut TaskDocument,
        event: RuntimeEvent,
    ) -> TaskMutationSummary {
        let mut summary = TaskMutationSummary::default();
        match event {
            RuntimeEvent::TurnStart { .. } => {
                // begin_turn is the explicit API; TurnStart just marks timing.
            }
            RuntimeEvent::TranscriptLine { line } => {
                if let Some(active) = doc.active_turn.as_mut() {
                    let step_id = Self::alloc_step(&mut doc.meta.next_step_id);
                    active.entries.push(TurnEntry::SystemNotice {
                        step_id,
                        message: line,
                        severity: NoticeSeverity::Info,
                    });
                    summary.active_turn_changed = true;
                    summary.appended_rows += 1;
                }
            }
            RuntimeEvent::TranscriptBlockStart { index, block } => {
                if let Some(active) = doc.active_turn.as_mut() {
                    let (phase, content, collapsed) = match &block {
                        StreamBlock::Thinking { content, collapsed } => {
                            (AssistantPhase::Thinking, content.clone(), *collapsed)
                        }
                        StreamBlock::FinalText { content } => {
                            (AssistantPhase::Final, content.clone(), false)
                        }
                        // Tool call / result blocks produce separate TurnEntry variants
                        // when the corresponding ToolCall / ToolResult events arrive.
                        _ => return summary,
                    };
                    let step_id = Self::alloc_step(&mut doc.meta.next_step_id);
                    active.entries.push(TurnEntry::AssistantBlock {
                        step_id,
                        block: AssistantBlockEntry {
                            block_index: index,
                            phase,
                            content,
                            collapsed,
                            streaming: true,
                        },
                    });
                    summary.active_turn_changed = true;
                    summary.appended_rows += 1;
                }
            }
            RuntimeEvent::TranscriptBlockDelta { index, delta } => {
                if let Some(active) = doc.active_turn.as_mut() {
                    Self::update_block_content(active, index, |entry| {
                        entry.content.push_str(&delta);
                    });
                    if active.ttft_ms.is_none() {
                        active.ttft_ms = Some(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0)
                                .saturating_sub(active.started_at_ms),
                        );
                    }
                    summary.active_turn_changed = true;
                }
            }
            RuntimeEvent::TranscriptBlockComplete { index } => {
                if let Some(active) = doc.active_turn.as_mut() {
                    Self::update_block_content(active, index, |entry| {
                        entry.streaming = false;
                    });
                    summary.active_turn_changed = true;
                }
            }
            RuntimeEvent::ToolCall {
                id,
                name,
                arguments,
            } => {
                if let Some(active) = doc.active_turn.as_mut() {
                    let step_id = Self::alloc_step(&mut doc.meta.next_step_id);
                    active.entries.push(TurnEntry::ToolCall {
                        step_id,
                        id,
                        name,
                        input: arguments,
                        status: ToolStatus::Pending,
                    });
                    summary.active_turn_changed = true;
                    summary.appended_rows += 1;
                    summary.selected_step_id = Some(doc.meta.next_step_id.saturating_sub(1));
                }
            }
            RuntimeEvent::ToolResult {
                tool_call_id,
                tool_name,
                is_error,
                output,
            } => {
                if let Some(active) = doc.active_turn.as_mut() {
                    // Advance the matching ToolCall status.
                    for entry in &mut active.entries {
                        if let TurnEntry::ToolCall { id, status, .. } = entry {
                            if *id == tool_call_id {
                                *status = if is_error {
                                    ToolStatus::Error
                                } else {
                                    ToolStatus::Complete
                                };
                                break;
                            }
                        }
                    }
                    let step_id = Self::alloc_step(&mut doc.meta.next_step_id);
                    active.entries.push(TurnEntry::ToolResult {
                        step_id,
                        tool_call_id,
                        tool_name,
                        output,
                        is_error,
                    });
                    summary.active_turn_changed = true;
                    summary.appended_rows += 1;
                }
            }
            RuntimeEvent::ApprovalRequest {
                capability,
                scope,
                tool_name,
            } => {
                if let Some(active) = doc.active_turn.as_mut() {
                    if let (Ok(cap), Ok(scp)) = (
                        capability.parse::<Capability>(),
                        scope.parse::<ApprovalScope>(),
                    ) {
                        let step_id = Self::alloc_step(&mut doc.meta.next_step_id);
                        let approval = ApprovalDocument {
                            step_id,
                            capability: cap,
                            scope: scp,
                            tool_name,
                            input_preview: String::new(),
                        };
                        active.pending_approval = Some(approval.clone());
                        active
                            .entries
                            .push(TurnEntry::ApprovalRequest { step_id, approval });
                        doc.meta.status = TaskStatus::AwaitingApproval;
                        summary.active_turn_changed = true;
                        summary.approval_changed = true;
                        summary.task_status_changed = true;
                    }
                }
            }
            RuntimeEvent::ApprovalResolved {
                capability,
                scope,
                approved,
            } => {
                if let Some(active) = doc.active_turn.as_mut() {
                    if let (Ok(cap), Ok(scp)) = (
                        capability.parse::<Capability>(),
                        scope.parse::<ApprovalScope>(),
                    ) {
                        active.pending_approval = None;
                        let step_id = Self::alloc_step(&mut doc.meta.next_step_id);
                        active.entries.push(TurnEntry::ApprovalResolved {
                            step_id,
                            capability: cap,
                            scope: scp,
                            approved,
                        });
                        doc.meta.status = TaskStatus::Running;
                        summary.active_turn_changed = true;
                        summary.approval_changed = true;
                        summary.task_status_changed = true;
                    }
                }
            }
            RuntimeEvent::ValidationResult { .. } => {
                // Validation results are emitted after a turn; treat them as
                // informational notices on the active turn if one is open.
            }
            RuntimeEvent::TurnEnd {
                status,
                changed_files,
                ..
            } => {
                if let Some(active) = doc.active_turn.as_mut() {
                    for f in changed_files {
                        active.changed_files.insert(f);
                    }
                    let _ = status; // outcome is provided via finish_turn
                }
            }
            RuntimeEvent::Error {
                message,
                recoverable,
                ..
            } => {
                doc.last_error = Some(TaskErrorState {
                    message,
                    recoverable,
                });
                if !recoverable {
                    doc.meta.status = TaskStatus::Failed;
                    summary.task_status_changed = true;
                }
            }
            RuntimeEvent::MaxTurnsReached { .. } => {
                doc.meta.status = TaskStatus::MaxTurnsReached;
                summary.task_status_changed = true;
            }
        }
        doc.meta.updated_at_ms = now_millis();
        summary
    }

    /// Close the active turn and push it onto `completed_turns`.
    pub fn finish_turn(
        &self,
        doc: &mut TaskDocument,
        outcome: TurnOutcome,
        tokens: TurnTokens,
        completed_at_ms: u64,
    ) -> TaskMutationSummary {
        let mut summary = TaskMutationSummary::default();
        let Some(active) = doc.active_turn.take() else {
            return summary;
        };
        let new_status = match &outcome {
            TurnOutcome::Completed => TaskStatus::Ready,
            TurnOutcome::Failed { .. } => TaskStatus::Failed,
            TurnOutcome::Cancelled => TaskStatus::Cancelled,
            TurnOutcome::MaxTurnsReached => TaskStatus::MaxTurnsReached,
        };
        doc.meta.status = new_status;
        doc.meta.updated_at_ms = completed_at_ms;
        summary.task_status_changed = true;
        summary.active_turn_changed = true;
        let completed = TurnDocument {
            turn_index: active.turn_index,
            input: active.input,
            entries: active.entries,
            outcome,
            changed_files: active.changed_files.into_iter().collect(),
            command_history: active.command_history,
            tokens,
            started_at_ms: active.started_at_ms,
            completed_at_ms,
            ttft_ms: active.ttft_ms,
            timings: active.timings,
        };
        doc.completed_turns.push(completed);
        summary
    }

    /// Produce a [`TaskState`] snapshot suitable for persistence.  This is a
    /// lossy projection: completed turns are summarised as
    /// [`TurnEvidenceState`] records.
    pub fn persistable_snapshot(&self, doc: &TaskDocument) -> TaskState {
        use crate::runtime::task_state::{CacheUsageStats, ConversationCheckpoint};
        use crate::turn_evidence::TurnEvidenceState;

        let turns: Vec<TurnEvidenceState> = doc
            .completed_turns
            .iter()
            .map(|t| TurnEvidenceState {
                input: t.input.clone(),
                response: Self::extract_final_text(&t.entries),
                changed_files: t.changed_files.clone(),
                command_history: t.command_history.clone(),
                tool_invocations: Self::extract_tool_invocations(&t.entries),
                tokens: t.tokens,
            })
            .collect();

        TaskState {
            id: doc.meta.id.clone(),
            status: doc.meta.status.clone(),
            parent_task_id: doc.meta.parent_task_id.clone(),
            agent_id: doc.meta.agent_id.clone(),
            worktree_path: doc.meta.worktree_path.clone(),
            started_at: doc.meta.started_at_ms,
            updated_at: doc.meta.updated_at_ms,
            last_heartbeat: doc.meta.last_heartbeat_ms,
            handoff_summary: None,
            active_grants: doc.meta.active_grants.clone(),
            changed_files: doc
                .completed_turns
                .iter()
                .flat_map(|t| t.changed_files.iter().map(std::path::PathBuf::from))
                .collect(),
            command_history: doc
                .completed_turns
                .iter()
                .flat_map(|t| t.command_history.iter().cloned())
                .collect(),
            conversation_snapshot: ConversationCheckpoint {
                message_count: doc.completed_turns.len(),
                summary: String::new(),
            },
            interrupted_sessions: Vec::new(),
            branch_name: doc.meta.branch_name.clone(),
            instructions_path: doc.meta.instructions_path.clone(),
            turns,
            plan: None,
            session_notes: doc.session_notes.clone(),
            context_compaction: doc.context_compaction.clone(),
            cache_usage: CacheUsageStats::default(),
            session_tasks: doc.session_tasks.clone(),
        }
    }

    /// Reconstruct a [`TaskDocument`] from a persisted [`TaskState`].  The
    /// active turn is always absent after restore — the runtime must call
    /// `begin_turn` once the first user message arrives.
    pub fn restore_from_snapshot(&self, snapshot: TaskState) -> TaskDocument {
        use crate::turn_evidence::TurnEvidenceState;

        let completed_turns: Vec<TurnDocument> = snapshot
            .turns
            .iter()
            .enumerate()
            .map(|(i, ev): (usize, &TurnEvidenceState)| TurnDocument {
                turn_index: i,
                input: ev.input.clone(),
                entries: Self::entries_from_evidence(i as u64, ev),
                outcome: TurnOutcome::Completed,
                changed_files: ev.changed_files.clone(),
                command_history: ev.command_history.clone(),
                tokens: ev.tokens,
                started_at_ms: 0,
                completed_at_ms: 0,
                ttft_ms: None,
                timings: None,
            })
            .collect();

        let next_step_id = completed_turns
            .iter()
            .flat_map(|t| t.entries.iter())
            .map(|e| step_id_of(e).saturating_add(1))
            .max()
            .unwrap_or(1);

        TaskDocument {
            meta: TaskMeta {
                id: snapshot.id,
                status: snapshot.status,
                parent_task_id: snapshot.parent_task_id,
                agent_id: snapshot.agent_id,
                worktree_path: snapshot.worktree_path,
                branch_name: snapshot.branch_name,
                instructions_path: snapshot.instructions_path,
                model_name: String::new(),
                model_backend: ModelBackendKind::LocalRuntime,
                model_url: String::new(),
                started_at_ms: snapshot.started_at,
                updated_at_ms: snapshot.updated_at,
                last_heartbeat_ms: snapshot.last_heartbeat,
                active_grants: snapshot.active_grants,
                next_step_id,
            },
            completed_turns,
            active_turn: None,
            session_notes: snapshot.session_notes,
            context_compaction: snapshot.context_compaction,
            session_tasks: snapshot.session_tasks,
            last_error: None,
        }
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    fn alloc_step(counter: &mut u64) -> u64 {
        let id = *counter;
        *counter = counter.saturating_add(1);
        id
    }

    fn update_block_content(
        active: &mut ActiveTurnDocument,
        block_index: usize,
        f: impl Fn(&mut AssistantBlockEntry),
    ) {
        for entry in active.entries.iter_mut().rev() {
            if let TurnEntry::AssistantBlock { block, .. } = entry {
                if block.block_index == block_index {
                    f(block);
                    return;
                }
            }
        }
    }

    fn extract_final_text(entries: &[TurnEntry]) -> String {
        entries
            .iter()
            .filter_map(|e| {
                if let TurnEntry::AssistantBlock { block, .. } = e {
                    if block.phase == AssistantPhase::Final {
                        return Some(block.content.as_str());
                    }
                }
                None
            })
            .collect::<Vec<_>>()
            .join("")
    }

    fn extract_tool_invocations(
        entries: &[TurnEntry],
    ) -> Vec<crate::turn_evidence::ToolInvocationSummary> {
        entries
            .iter()
            .filter_map(|e| {
                if let TurnEntry::ToolCall {
                    step_id,
                    name,
                    status,
                    ..
                } = e
                {
                    Some(crate::turn_evidence::ToolInvocationSummary {
                        step_id: *step_id,
                        name: name.clone(),
                        outcome: format!("{status:?}").to_lowercase(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    fn entries_from_evidence(
        base_step: u64,
        ev: &crate::turn_evidence::TurnEvidenceState,
    ) -> Vec<TurnEntry> {
        let mut entries = Vec::new();
        let mut step = base_step * 1000;

        entries.push(TurnEntry::UserInput {
            step_id: step,
            text: ev.input.clone(),
        });
        step += 1;

        if !ev.response.is_empty() {
            entries.push(TurnEntry::AssistantBlock {
                step_id: step,
                block: AssistantBlockEntry {
                    block_index: 0,
                    phase: AssistantPhase::Final,
                    content: ev.response.clone(),
                    collapsed: false,
                    streaming: false,
                },
            });
        }

        for inv in &ev.tool_invocations {
            step += 1;
            entries.push(TurnEntry::ToolCall {
                step_id: inv.step_id.max(step),
                id: format!("restored-{}", inv.step_id),
                name: inv.name.clone(),
                input: serde_json::Value::Object(Default::default()),
                status: ToolStatus::Complete,
            });
        }

        entries
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers for Capability / ApprovalScope from string
// ---------------------------------------------------------------------------

impl std::str::FromStr for Capability {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "read_file" => Ok(Capability::ReadFile),
            "write_file" => Ok(Capability::WriteFile),
            "apply_patch" => Ok(Capability::ApplyPatch),
            "run_command" => Ok(Capability::RunCommand),
            "mcp_tool" => Ok(Capability::McpTool),
            "network" => Ok(Capability::Network),
            "browser" => Ok(Capability::Browser),
            _ => Err(format!("unknown capability: {s}")),
        }
    }
}

impl std::str::FromStr for ApprovalScope {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "once" => Ok(ApprovalScope::Once),
            "task" => Ok(ApprovalScope::Task),
            "session" => Ok(ApprovalScope::Session),
            _ => Err(format!("unknown scope: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn step_id_of(entry: &TurnEntry) -> u64 {
    match entry {
        TurnEntry::UserInput { step_id, .. } => *step_id,
        TurnEntry::AssistantBlock { step_id, .. } => *step_id,
        TurnEntry::ToolCall { step_id, .. } => *step_id,
        TurnEntry::ToolResult { step_id, .. } => *step_id,
        TurnEntry::ApprovalRequest { step_id, .. } => *step_id,
        TurnEntry::ApprovalResolved { step_id, .. } => *step_id,
        TurnEntry::CommandSession { step_id, .. } => *step_id,
        TurnEntry::SystemNotice { step_id, .. } => *step_id,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::task_state::TaskStatus;

    fn test_meta() -> TaskMeta {
        TaskMeta {
            id: "test-task-01".to_string(),
            status: TaskStatus::Ready,
            parent_task_id: None,
            agent_id: None,
            worktree_path: None,
            branch_name: None,
            instructions_path: None,
            model_name: "test-model".to_string(),
            model_backend: ModelBackendKind::LocalRuntime,
            model_url: "https://api.example.com".to_string(),
            started_at_ms: Some(1000),
            updated_at_ms: 1000,
            last_heartbeat_ms: None,
            active_grants: HashMap::new(),
            next_step_id: 1,
        }
    }

    #[test]
    fn begin_task_produces_empty_document() {
        let reducer = TaskDocumentReducer::new();
        let doc = reducer.begin_task(test_meta());
        assert!(doc.completed_turns.is_empty());
        assert!(doc.active_turn.is_none());
        assert_eq!(doc.meta.id, "test-task-01");
    }

    #[test]
    fn begin_turn_opens_active_turn_with_user_input_entry() {
        let reducer = TaskDocumentReducer::new();
        let mut doc = reducer.begin_task(test_meta());
        reducer.begin_turn(&mut doc, "hello".to_string(), 2000, TurnToolPolicy::Default);
        let active = doc.active_turn.as_ref().expect("active turn");
        assert_eq!(active.turn_index, 0);
        assert_eq!(active.input, "hello");
        assert_eq!(active.entries.len(), 1);
        matches!(active.entries[0], TurnEntry::UserInput { .. });
    }

    #[test]
    fn finish_turn_moves_active_turn_to_completed() {
        let reducer = TaskDocumentReducer::new();
        let mut doc = reducer.begin_task(test_meta());
        reducer.begin_turn(&mut doc, "q".to_string(), 1000, TurnToolPolicy::Default);
        let summary = reducer.finish_turn(
            &mut doc,
            TurnOutcome::Completed,
            TurnTokens::default(),
            2000,
        );
        assert!(doc.active_turn.is_none());
        assert_eq!(doc.completed_turns.len(), 1);
        assert!(summary.active_turn_changed);
        assert!(summary.task_status_changed);
        assert_eq!(doc.meta.status, TaskStatus::Ready);
    }

    #[test]
    fn apply_tool_call_event_appends_entry() {
        let reducer = TaskDocumentReducer::new();
        let mut doc = reducer.begin_task(test_meta());
        reducer.begin_turn(&mut doc, "q".to_string(), 1000, TurnToolPolicy::Default);
        let summary = reducer.apply_runtime_event(
            &mut doc,
            RuntimeEvent::ToolCall {
                id: "tc-01".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "src/main.rs"}),
            },
        );
        let active = doc.active_turn.as_ref().unwrap();
        let has_tool = active
            .entries
            .iter()
            .any(|e| matches!(e, TurnEntry::ToolCall { .. }));
        assert!(has_tool);
        assert!(summary.active_turn_changed);
    }

    #[test]
    fn tool_result_advances_tool_call_status() {
        let reducer = TaskDocumentReducer::new();
        let mut doc = reducer.begin_task(test_meta());
        reducer.begin_turn(&mut doc, "q".to_string(), 1000, TurnToolPolicy::Default);
        reducer.apply_runtime_event(
            &mut doc,
            RuntimeEvent::ToolCall {
                id: "tc-01".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({}),
            },
        );
        reducer.apply_runtime_event(
            &mut doc,
            RuntimeEvent::ToolResult {
                tool_call_id: "tc-01".to_string(),
                tool_name: Some("read_file".to_string()),
                is_error: false,
                output: "file contents".to_string(),
            },
        );
        let active = doc.active_turn.as_ref().unwrap();
        let call_status = active.entries.iter().find_map(|e| {
            if let TurnEntry::ToolCall { id, status, .. } = e {
                if id == "tc-01" {
                    return Some(status.clone());
                }
            }
            None
        });
        assert_eq!(call_status, Some(ToolStatus::Complete));
    }

    #[test]
    fn snapshot_roundtrip_preserves_turn_count() {
        let reducer = TaskDocumentReducer::new();
        let mut doc = reducer.begin_task(test_meta());
        reducer.begin_turn(
            &mut doc,
            "turn one".to_string(),
            1000,
            TurnToolPolicy::Default,
        );
        reducer.finish_turn(
            &mut doc,
            TurnOutcome::Completed,
            TurnTokens::default(),
            2000,
        );
        reducer.begin_turn(
            &mut doc,
            "turn two".to_string(),
            3000,
            TurnToolPolicy::Default,
        );
        reducer.finish_turn(
            &mut doc,
            TurnOutcome::Completed,
            TurnTokens::default(),
            4000,
        );
        let snapshot = reducer.persistable_snapshot(&doc);
        assert_eq!(snapshot.turns.len(), 2);
        let restored = reducer.restore_from_snapshot(snapshot);
        assert_eq!(restored.completed_turns.len(), 2);
    }

    #[test]
    fn error_event_sets_error_state_and_status() {
        let reducer = TaskDocumentReducer::new();
        let mut doc = reducer.begin_task(test_meta());
        let summary = reducer.apply_runtime_event(
            &mut doc,
            RuntimeEvent::Error {
                code: "E001".to_string(),
                message: "something went wrong".to_string(),
                recoverable: false,
            },
        );
        assert!(doc.last_error.is_some());
        assert_eq!(doc.meta.status, TaskStatus::Failed);
        assert!(summary.task_status_changed);
    }
}
