use crate::runtime::json_handoff::RuntimeEvent;
use crate::runtime::session_task::now_millis;
use crate::runtime::task_state::TaskStatus;
use crate::runtime::{ApprovalScope, Capability};
use crate::state::{StreamBlock, ToolStatus, TurnToolPolicy};
use crate::usage::TurnTokens;

use super::{
    ActiveTurnDocument, ApprovalDocument, AssistantBlockEntry, AssistantPhase, NoticeSeverity,
    TaskDocument, TaskErrorState, TaskMeta, TurnDocument, TurnEntry, TurnOutcome,
};

/// Stateless reducer that applies [`RuntimeEvent`] mutations to a
/// [`TaskDocument`] and produces snapshot adapters compatible with the
/// existing [`crate::runtime::TaskState`] persistence format.
#[derive(Debug, Default)]
pub struct TaskDocumentReducer;

/// Summary of what changed after a single reducer call. Callers use this to
/// drive incremental UI updates without re-rendering the whole document.
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

    /// Open a new active turn. Panics if `doc.active_turn` is already set
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
        let step_id = Self::alloc_step(&mut doc.meta.next_step_id);

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
            command_sessions: Default::default(),
            changed_files: Default::default(),
            command_history: Vec::new(),
            tool_policy,
            cancel_pending: false,
        });
    }

    /// Apply one [`RuntimeEvent`] to the document.
    ///
    /// Events that do not affect the active turn (for example, events produced
    /// before `begin_turn` or after `finish_turn`) are ignored so callers do
    /// not need to gate every call.
    pub fn apply_runtime_event(
        &self,
        doc: &mut TaskDocument,
        event: RuntimeEvent,
    ) -> TaskMutationSummary {
        let mut summary = TaskMutationSummary::default();

        match event {
            RuntimeEvent::TurnStart { .. } => {}
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
                    let (phase, content, collapsed) = match block {
                        StreamBlock::Thinking { content, collapsed } => {
                            (AssistantPhase::Thinking, content, collapsed)
                        }
                        StreamBlock::FinalText { content } => {
                            (AssistantPhase::Final, content, false)
                        }
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
                        active.ttft_ms = Some(now_millis().saturating_sub(active.started_at_ms));
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
                    summary.selected_step_id = Some(step_id);
                }
            }
            RuntimeEvent::ToolResult {
                tool_call_id,
                tool_name,
                is_error,
                output,
            } => {
                if let Some(active) = doc.active_turn.as_mut() {
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
                    if let (Ok(capability), Ok(scope)) = (
                        capability.parse::<Capability>(),
                        scope.parse::<ApprovalScope>(),
                    ) {
                        let step_id = Self::alloc_step(&mut doc.meta.next_step_id);
                        let approval = ApprovalDocument {
                            step_id,
                            capability,
                            scope,
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
                    if let (Ok(capability), Ok(scope)) = (
                        capability.parse::<Capability>(),
                        scope.parse::<ApprovalScope>(),
                    ) {
                        active.pending_approval = None;
                        let step_id = Self::alloc_step(&mut doc.meta.next_step_id);
                        active.entries.push(TurnEntry::ApprovalResolved {
                            step_id,
                            capability,
                            scope,
                            approved,
                        });

                        if approved {
                            match scope {
                                ApprovalScope::Once => {}
                                ApprovalScope::Task | ApprovalScope::Session => {
                                    doc.meta.active_grants.insert(capability, scope);
                                }
                            }
                        } else {
                            doc.meta.active_grants.remove(&capability);
                        }

                        doc.meta.status = TaskStatus::Running;
                        summary.active_turn_changed = true;
                        summary.approval_changed = true;
                        summary.task_status_changed = true;
                    }
                }
            }
            RuntimeEvent::ValidationResult { .. } => {}
            RuntimeEvent::TurnEnd {
                status: _,
                changed_files,
                ..
            } => {
                if let Some(active) = doc.active_turn.as_mut() {
                    let before = active.changed_files.len();
                    for path in changed_files {
                        active.changed_files.insert(path);
                    }
                    summary.active_turn_changed = active.changed_files.len() != before;
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
        let Some(mut active) = doc.active_turn.take() else {
            return summary;
        };

        // Clear streaming flags so completed turn entries render without a
        // live-typing cursor.
        for entry in &mut active.entries {
            if let TurnEntry::AssistantBlock { block, .. } = entry {
                block.streaming = false;
            }
        }

        doc.meta.status = match &outcome {
            TurnOutcome::Completed => TaskStatus::Ready,
            TurnOutcome::Failed { .. } => TaskStatus::Failed,
            TurnOutcome::Cancelled => TaskStatus::Cancelled,
            TurnOutcome::MaxTurnsReached => TaskStatus::MaxTurnsReached,
        };
        doc.meta.updated_at_ms = completed_at_ms;

        summary.task_status_changed = true;
        summary.active_turn_changed = true;
        doc.completed_turns.push(TurnDocument {
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
        });
        summary
    }

    pub(super) fn alloc_step(counter: &mut u64) -> u64 {
        let id = *counter;
        *counter = counter.saturating_add(1);
        id
    }

    fn update_block_content(
        active: &mut ActiveTurnDocument,
        block_index: usize,
        mutate: impl Fn(&mut AssistantBlockEntry),
    ) {
        for entry in active.entries.iter_mut().rev() {
            if let TurnEntry::AssistantBlock { block, .. } = entry {
                if block.block_index == block_index {
                    mutate(block);
                    return;
                }
            }
        }
    }
}
