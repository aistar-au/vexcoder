use crate::runtime::json_handoff::RuntimeEvent;
use crate::runtime::session_task::now_millis;
use crate::runtime::task_state::TaskStatus;
use crate::runtime::{ApprovalScope, Capability};
use crate::state::{StreamBlock, PulseToolPolicy};
use crate::usage::PulseTokens;

use super::{
    ActiveTurnDocument, ApprovalDocument, AssistantBlockEntry, AssistantPhase, NoticeSeverity,
    TaskDocument, TaskErrorState, TaskInfo, TurnDocument, PulseEntry, PulseOutcome,
};

#[derive(Debug, Default)]
pub struct TaskDocumentCondenser;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskMutationSummary {
    pub task_status_changed: bool,
    pub active_turn_changed: bool,
    pub appended_rows: usize,
    pub selected_step_id: Option<u64>,
    pub approval_changed: bool,
}

impl TaskDocumentCondenser {
    pub fn new() -> Self {
        Self
    }

    pub fn begin_task(&self, info: TaskInfo) -> TaskDocument {
        TaskDocument {
            info,
            completed_turns: Vec::new(),
            active_pulse: None,
            session_notes: Vec::new(),
            context_compaction: Vec::new(),
            session_tasks: Vec::new(),
            last_error: None,
        }
    }

    pub fn begin_turn(
        &self,
        doc: &mut TaskDocument,
        input: String,
        started_at_ms: u64,
        tool_policy: PulseToolPolicy,
    ) {
        assert!(
            doc.active_pulse.is_none(),
            "begin_turn called while an active pulse is already open"
        );

        let turn_index = doc.completed_turns.len();
        let step_id = Self::alloc_step(&mut doc.info.next_step_id);

        doc.info.status = TaskStatus::Running;
        doc.info.updated_at_ms = started_at_ms;
        doc.active_pulse = Some(ActiveTurnDocument {
            turn_index,
            input: input.clone(),
            entries: vec![PulseEntry::UserInput {
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

    pub fn apply_runtime_event(
        &self,
        doc: &mut TaskDocument,
        event: RuntimeEvent,
    ) -> TaskMutationSummary {
        let mut summary = TaskMutationSummary::default();

        match event {
            RuntimeEvent::PulseStart { .. } => {}
            RuntimeEvent::TranscriptLine { line } => {
                if let Some(active) = doc.active_pulse.as_mut() {
                    let step_id = Self::alloc_step(&mut doc.info.next_step_id);
                    active.entries.push(PulseEntry::SystemNotice {
                        step_id,
                        message: line,
                        severity: NoticeSeverity::Info,
                    });
                    summary.active_turn_changed = true;
                    summary.appended_rows += 1;
                }
            }
            RuntimeEvent::TranscriptBlockStart { index, block } => {
                if let Some(active) = doc.active_pulse.as_mut() {
                    let (phase, content, collapsed) = match block {
                        StreamBlock::Thinking { content, collapsed } => {
                            (AssistantPhase::Thinking, content, collapsed)
                        }
                        StreamBlock::FinalText { content } => {
                            (AssistantPhase::Final, content, false)
                        }
                        _ => return summary,
                    };
                    let step_id = Self::alloc_step(&mut doc.info.next_step_id);
                    active.entries.push(PulseEntry::AssistantBlock {
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
                if let Some(active) = doc.active_pulse.as_mut() {
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
                if let Some(active) = doc.active_pulse.as_mut() {
                    Self::update_block_content(active, index, |entry| {
                        entry.streaming = false;
                    });
                    summary.active_turn_changed = true;
                }
            }
            RuntimeEvent::ToolCallStatusUpdated {
                tool_call_id,
                status,
            } => {
                if let Some(active) = doc.active_pulse.as_mut() {
                    for entry in &mut active.entries {
                        if let PulseEntry::ToolCall {
                            id,
                            status: current_status,
                            ..
                        } = entry
                            && *id == tool_call_id
                        {
                            *current_status = status;
                            summary.active_turn_changed = true;
                            break;
                        }
                    }
                }
            }
            RuntimeEvent::TranscriptBlockPhaseUpdated {
                index,
                phase,
                streaming,
            } => {
                if let Some(active) = doc.active_pulse.as_mut() {
                    Self::update_block_content(active, index, |entry| {
                        entry.phase = phase.clone();
                        entry.streaming = streaming;
                    });
                    summary.active_turn_changed = true;
                }
            }
            RuntimeEvent::ToolCallStarted {
                tool_call_id,
                tool_name,
                arguments,
                status,
                ..
            } => {
                if let Some(active) = doc.active_pulse.as_mut() {
                    let step_id = Self::alloc_step(&mut doc.info.next_step_id);
                    active.entries.push(PulseEntry::ToolCall {
                        step_id,
                        id: tool_call_id,
                        name: tool_name,
                        input: arguments,
                        status,
                    });
                    summary.active_turn_changed = true;
                    summary.appended_rows += 1;
                    summary.selected_step_id = Some(step_id);
                }
            }
            RuntimeEvent::ToolCallArgumentsDelta { .. } => {}
            RuntimeEvent::ToolCallCompleted {
                tool_call_id,
                tool_name,
                output,
                status,
                ..
            } => {
                if let Some(active) = doc.active_pulse.as_mut() {
                    for entry in &mut active.entries {
                        if let PulseEntry::ToolCall {
                            id,
                            status: entry_status,
                            ..
                        } = entry
                            && *id == tool_call_id
                        {
                            *entry_status = status.clone();
                            break;
                        }
                    }

                    let step_id = Self::alloc_step(&mut doc.info.next_step_id);
                    active.entries.push(PulseEntry::ToolResult {
                        step_id,
                        tool_call_id,
                        tool_name,
                        output,
                        is_error: false,
                    });
                    summary.active_turn_changed = true;
                    summary.appended_rows += 1;
                }
            }
            RuntimeEvent::ToolCallFailed {
                tool_call_id,
                tool_name,
                output,
                status,
                ..
            } => {
                if let Some(active) = doc.active_pulse.as_mut() {
                    for entry in &mut active.entries {
                        if let PulseEntry::ToolCall {
                            id,
                            status: entry_status,
                            ..
                        } = entry
                            && *id == tool_call_id
                        {
                            *entry_status = status.clone();
                            break;
                        }
                    }

                    let step_id = Self::alloc_step(&mut doc.info.next_step_id);
                    active.entries.push(PulseEntry::ToolResult {
                        step_id,
                        tool_call_id,
                        tool_name,
                        output,
                        is_error: true,
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
                if let Some(active) = doc.active_pulse.as_mut()
                    && let (Ok(capability), Ok(scope)) = (
                        capability.parse::<Capability>(),
                        scope.parse::<ApprovalScope>(),
                    )
                {
                    let step_id = Self::alloc_step(&mut doc.info.next_step_id);
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
                        .push(PulseEntry::ApprovalRequest { step_id, approval });
                    doc.info.status = TaskStatus::AwaitingApproval;
                    summary.active_turn_changed = true;
                    summary.approval_changed = true;
                    summary.task_status_changed = true;
                }
            }
            RuntimeEvent::ApprovalResolved {
                capability,
                scope,
                approved,
            } => {
                if let Some(active) = doc.active_pulse.as_mut()
                    && let (Ok(capability), Ok(scope)) = (
                        capability.parse::<Capability>(),
                        scope.parse::<ApprovalScope>(),
                    )
                {
                    active.pending_approval = None;
                    let step_id = Self::alloc_step(&mut doc.info.next_step_id);
                    active.entries.push(PulseEntry::ApprovalResolved {
                        step_id,
                        capability,
                        scope,
                        approved,
                    });

                    if approved {
                        match scope {
                            ApprovalScope::Once => {}
                            ApprovalScope::Task | ApprovalScope::Session => {
                                doc.info.active_grants.insert(capability, scope);
                            }
                        }
                    } else {
                        doc.info.active_grants.remove(&capability);
                    }

                    doc.info.status = TaskStatus::Running;
                    summary.active_turn_changed = true;
                    summary.approval_changed = true;
                    summary.task_status_changed = true;
                }
            }
            RuntimeEvent::ValidationResult { .. } => {}
            RuntimeEvent::ServerMetadata { metadata } => {
                if let Some(active) = doc.active_pulse.as_mut() {
                    if let Some(prompt_progress) = metadata.prompt_progress {
                        active.prompt_progress = Some(prompt_progress);
                    }
                    if let Some(timings) = metadata.timings {
                        active.timings = Some(timings);
                    }
                    summary.active_turn_changed = true;
                }
            }
            RuntimeEvent::UsageUpdated { .. } => {}
            RuntimeEvent::PulseEnd {
                status: _,
                changed_files,
                ..
            } => {
                if let Some(active) = doc.active_pulse.as_mut() {
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
                    doc.info.status = TaskStatus::Failed;
                    summary.task_status_changed = true;
                }
            }
            RuntimeEvent::MaxTurnsReached { .. } => {
                doc.info.status = TaskStatus::MaxTurnsReached;
                summary.task_status_changed = true;
            }
        }

        doc.info.updated_at_ms = now_millis();
        summary
    }

    pub fn finish_turn(
        &self,
        doc: &mut TaskDocument,
        outcome: PulseOutcome,
        tokens: PulseTokens,
        completed_at_ms: u64,
    ) -> TaskMutationSummary {
        let mut summary = TaskMutationSummary::default();
        let Some(mut active) = doc.active_pulse.take() else {
            return summary;
        };

        for entry in &mut active.entries {
            if let PulseEntry::AssistantBlock { block, .. } = entry {
                block.streaming = false;
            }
        }

        doc.info.status = match &outcome {
            PulseOutcome::Completed => TaskStatus::Ready,
            PulseOutcome::Failed { .. } => TaskStatus::Failed,
            PulseOutcome::Cancelled => TaskStatus::Cancelled,
            PulseOutcome::MaxTurnsReached => TaskStatus::MaxTurnsReached,
        };
        doc.info.updated_at_ms = completed_at_ms;

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
            if let PulseEntry::AssistantBlock { block, .. } = entry
                && block.block_index == block_index
            {
                mutate(block);
                return;
            }
        }
    }
}
