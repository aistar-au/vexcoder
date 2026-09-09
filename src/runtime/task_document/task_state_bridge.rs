use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::pulse_evidence::{ToolInvocationSummary, TurnEvidenceState};
use crate::runtime::ModelBackendKind;
use crate::runtime::task_state::{
    CacheUsageStats, ConversationCheckpoint, LiveJoinEntry, PathChange, RecordedDecision,
    TaskState, WorkingSetRecord,
};
use crate::state::ToolStatus;

use super::{
    AssistantBlockEntry, AssistantPhase, PulseEntry, PulseOutcome, TaskDocument,
    TaskDocumentCondenser, TaskInfo, TurnDocument,
};

impl TaskDocumentCondenser {
    pub fn persistable_snapshot(&self, doc: &TaskDocument) -> TaskState {
        let pulses = doc
            .completed_turns
            .iter()
            .map(|pulse| TurnEvidenceState {
                input: pulse.input.clone(),
                response: extract_final_text(&pulse.entries),
                changed_files: pulse.changed_files.clone(),
                command_history: pulse.command_history.clone(),
                tool_invocations: extract_tool_invocations(&pulse.entries),
                tokens: pulse.tokens,
            })
            .collect();

        TaskState {
            id: doc.info.id.clone(),
            status: doc.info.status.clone(),
            parent_task_id: doc.info.parent_task_id.clone(),
            agent_id: doc.info.agent_id.clone(),
            worktree_path: doc.info.worktree_path.clone(),
            started_at: doc.info.started_at_ms,
            updated_at: doc.info.updated_at_ms,
            last_heartbeat: doc.info.last_heartbeat_ms,
            handoff_summary: None,
            active_grants: doc.info.active_grants.clone(),
            changed_files: doc
                .completed_turns
                .iter()
                .flat_map(|pulse| pulse.changed_files.iter().map(std::path::PathBuf::from))
                .collect(),
            command_history: doc
                .completed_turns
                .iter()
                .flat_map(|pulse| pulse.command_history.iter().cloned())
                .collect(),
            conversation_snapshot: ConversationCheckpoint {
                message_count: doc.completed_turns.len(),
                summary: String::new(),
            },
            interrupted_sessions: Vec::new(),
            branch_name: doc.info.branch_name.clone(),
            instructions_path: doc.info.instructions_path.clone(),
            pulses,
            plan: None,
            session_notes: doc.session_notes.clone(),
            context_compaction: doc.context_compaction.clone(),
            cache_usage: CacheUsageStats::default(),
            session_tasks: doc.session_tasks.clone(),
        }
    }

    pub fn restore_from_snapshot(&self, snapshot: TaskState) -> TaskDocument {
        let completed_turns: Vec<TurnDocument> = snapshot
            .pulses
            .iter()
            .enumerate()
            .map(|(turn_index, evidence)| TurnDocument {
                turn_index,
                input: evidence.input.clone(),
                entries: entries_from_evidence(turn_index as u64, evidence),
                outcome: PulseOutcome::Completed,
                changed_files: evidence.changed_files.clone(),
                command_history: evidence.command_history.clone(),
                tokens: evidence.tokens,
                started_at_ms: 0,
                completed_at_ms: 0,
                ttft_ms: None,
                timings: None,
            })
            .collect();

        let next_step_id = completed_turns
            .iter()
            .flat_map(|pulse| pulse.entries.iter())
            .map(entry_step_id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);

        TaskDocument {
            info: TaskInfo {
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
            active_pulse: None,
            session_notes: snapshot.session_notes,
            context_compaction: snapshot.context_compaction,
            session_tasks: snapshot.session_tasks,
            last_error: None,
        }
    }

    /// Project the live task document into a `WorkingSetRecord`.
    /// The condenser is the sole writer of that sidecar.
    pub fn project_working_set(&self, doc: &TaskDocument) -> WorkingSetRecord {
        let mut record = WorkingSetRecord::new(first_user_input(doc));
        record.constraints = doc
            .session_notes
            .iter()
            .map(|note| note.content.trim().to_string())
            .filter(|content| !content.is_empty())
            .collect();
        record.changed_paths = unique_changed_paths(doc);
        record.verified_results = verified_results(doc);
        record.active_plan = active_plan(doc);
        record.next_action = last_user_input(doc);
        record
    }

    pub fn write_working_set(
        &self,
        doc: &TaskDocument,
        dir: &Path,
    ) -> anyhow::Result<WorkingSetRecord> {
        let mut record = self.project_working_set(doc);
        match WorkingSetRecord::try_load(dir, &doc.info.id) {
            Ok(Some(prior)) => {
                record.retain_durable_objective(&prior);
                record.retain_referenced_decisions(&prior);
            }
            Ok(None) => {}
            Err(error) => return Err(error),
        }
        record.save(dir, &doc.info.id)?;
        Ok(record)
    }

    /// Condenser write of agent-join evidence. `RecordedDecision.source_reference`
    /// is the JoinIndex message id. Existing `objective` stays write-once.
    pub fn record_join_evidence(
        &self,
        dir: &Path,
        task_id: &str,
        entries: &[LiveJoinEntry],
    ) -> anyhow::Result<WorkingSetRecord> {
        let mut record = match WorkingSetRecord::try_load(dir, task_id) {
            Ok(Some(prior)) => prior,
            Ok(None) => WorkingSetRecord::new(""),
            Err(error) => return Err(error),
        };
        for entry in entries {
            if entry.id.trim().is_empty() {
                continue;
            }
            if record
                .decisions
                .iter()
                .any(|decision| decision.source_reference == entry.id)
            {
                continue;
            }
            record.decisions.push(RecordedDecision {
                rationale: entry.body.clone(),
                source_reference: entry.id.clone(),
            });
        }
        record.save(dir, task_id)?;
        Ok(record)
    }
}

fn first_user_input(doc: &TaskDocument) -> String {
    doc.completed_turns
        .iter()
        .map(|pulse| pulse.input.trim())
        .find(|input| !input.is_empty())
        .map(ToString::to_string)
        .or_else(|| nonempty_active_input(doc))
        .unwrap_or_default()
}

fn last_user_input(doc: &TaskDocument) -> String {
    nonempty_active_input(doc)
        .or_else(|| {
            doc.completed_turns
                .iter()
                .rev()
                .map(|pulse| pulse.input.trim())
                .find(|input| !input.is_empty())
                .map(ToString::to_string)
        })
        .unwrap_or_default()
}

fn nonempty_active_input(doc: &TaskDocument) -> Option<String> {
    doc.active_pulse.as_ref().and_then(|pulse| {
        let input = pulse.input.trim();
        (!input.is_empty()).then(|| input.to_string())
    })
}

fn unique_changed_paths(doc: &TaskDocument) -> Vec<PathChange> {
    let mut by_path = BTreeMap::new();
    for pulse in &doc.completed_turns {
        for path in &pulse.changed_files {
            let path = PathBuf::from(path);
            by_path.entry(path).or_default();
        }
    }
    if let Some(active) = &doc.active_pulse {
        for path in &active.changed_files {
            let path = PathBuf::from(path);
            by_path.entry(path).or_default();
        }
    }
    by_path
        .into_iter()
        .map(|(path, git_identity)| PathChange { path, git_identity })
        .collect()
}

fn verified_results(doc: &TaskDocument) -> Vec<String> {
    let mut results = Vec::new();
    for pulse in &doc.completed_turns {
        let text = extract_final_text(&pulse.entries);
        if !text.trim().is_empty() {
            results.push(text);
        }
    }
    if let Some(active) = &doc.active_pulse {
        let text = extract_final_text(&active.entries);
        if !text.trim().is_empty() {
            results.push(text);
        }
    }
    results
}

fn active_plan(doc: &TaskDocument) -> String {
    doc.session_notes
        .iter()
        .rev()
        .find_map(|note| {
            note.content
                .trim()
                .strip_prefix("[plan]")
                .map(|plan| plan.trim().to_string())
                .filter(|plan| !plan.is_empty())
        })
        .or_else(|| verified_results(doc).into_iter().next_back())
        .unwrap_or_default()
}

fn extract_final_text(entries: &[PulseEntry]) -> String {
    entries
        .iter()
        .filter_map(|entry| {
            if let PulseEntry::AssistantBlock { block, .. } = entry
                && block.phase == AssistantPhase::Final
            {
                return Some(block.content.as_str());
            }
            None
        })
        .collect::<Vec<_>>()
        .join("")
}

fn extract_tool_invocations(entries: &[PulseEntry]) -> Vec<ToolInvocationSummary> {
    let mut tool_calls = HashMap::new();
    let mut invocations = Vec::new();

    for entry in entries {
        match entry {
            PulseEntry::ToolCall {
                step_id,
                id,
                name,
                status,
                ..
            } => {
                tool_calls.insert(id.clone(), (*step_id, name.clone(), status.clone()));
            }
            PulseEntry::ToolResult {
                tool_call_id,
                output,
                is_error,
                ..
            } => {
                if let Some((step_id, name, _status)) = tool_calls.remove(tool_call_id) {
                    invocations.push(ToolInvocationSummary {
                        step_id,
                        name,
                        outcome: summarize_tool_outcome(output, *is_error).to_string(),
                    });
                }
            }
            _ => {}
        }
    }

    invocations.extend(tool_calls.into_values().map(|(step_id, name, status)| {
        ToolInvocationSummary {
            step_id,
            name,
            outcome: outcome_from_status(status).to_string(),
        }
    }));
    invocations.sort_by_key(|invocation| invocation.step_id);
    invocations
}

fn entries_from_evidence(base_step: u64, evidence: &TurnEvidenceState) -> Vec<PulseEntry> {
    let mut entries = Vec::new();
    let mut step = base_step.saturating_mul(1000);

    entries.push(PulseEntry::UserInput {
        step_id: step,
        text: evidence.input.clone(),
    });
    step = step.saturating_add(1);

    if !evidence.response.is_empty() {
        entries.push(PulseEntry::AssistantBlock {
            step_id: step,
            block: AssistantBlockEntry {
                block_index: 0,
                phase: AssistantPhase::Final,
                content: evidence.response.clone(),
                collapsed: false,
                streaming: false,
            },
        });
    }

    for invocation in &evidence.tool_invocations {
        step = step.saturating_add(1);
        let step_id = invocation.step_id.max(step);
        step = step_id;

        entries.push(PulseEntry::ToolCall {
            step_id,
            id: format!("restored-{}", invocation.step_id),
            name: invocation.name.clone(),
            input: serde_json::Value::Object(Default::default()),
            status: tool_status_from_outcome(&invocation.outcome),
        });
    }

    entries
}

fn summarize_tool_outcome(output: &str, is_error: bool) -> &'static str {
    if !is_error {
        return "ok";
    }

    let lowered = output.to_ascii_lowercase();
    if lowered.contains("denied") {
        "denied"
    } else if lowered.contains("cancel") {
        "cancelled"
    } else {
        "error"
    }
}

fn outcome_from_status(status: ToolStatus) -> &'static str {
    match status {
        ToolStatus::Complete => "ok",
        ToolStatus::Cancelled => "cancelled",
        ToolStatus::Error => "error",
        ToolStatus::Pending | ToolStatus::WaitingApproval | ToolStatus::Executing => "pending",
    }
}

fn tool_status_from_outcome(outcome: &str) -> ToolStatus {
    let lowered = outcome.to_ascii_lowercase();
    if lowered.contains("cancel") {
        ToolStatus::Cancelled
    } else if lowered.contains("denied") || lowered.contains("error") || lowered.contains("fail") {
        ToolStatus::Error
    } else if lowered == "ok" {
        ToolStatus::Complete
    } else {
        ToolStatus::Pending
    }
}

fn entry_step_id(entry: &PulseEntry) -> u64 {
    match entry {
        PulseEntry::UserInput { step_id, .. }
        | PulseEntry::AssistantBlock { step_id, .. }
        | PulseEntry::ToolCall { step_id, .. }
        | PulseEntry::ToolResult { step_id, .. }
        | PulseEntry::ApprovalRequest { step_id, .. }
        | PulseEntry::ApprovalResolved { step_id, .. }
        | PulseEntry::CommandSession { step_id, .. }
        | PulseEntry::SystemNotice { step_id, .. } => *step_id,
    }
}
