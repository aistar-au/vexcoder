use crate::runtime::json_handoff::ToolCallId;
use crate::runtime::tokio::sync::mpsc;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const DEFAULT_DELTA_ACCUMULATOR_MEMORY_WATERMARK_BYTES: usize = 256 * 1024 * 1024;
const MAX_STORED_PARTIAL_BYTES: usize = 65_536;
const MAX_TOOL_DELTA_QUEUE_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerDeltaEvent {
    ToolStart {
        tool_call_id: ToolCallId,
        task_id: String,
        name: String,
    },
    ToolDelta {
        tool_call_id: ToolCallId,
        task_id: String,
        partial_json: String,
    },
    ToolFinish {
        tool_call_id: ToolCallId,
        task_id: String,
    },
    PartialArgsTruncated {
        tool_call_id: ToolCallId,
        task_id: String,
        stored_bytes: usize,
    },
}

#[derive(Debug, Clone)]
pub struct ToolState {
    pub id: ToolCallId,
    pub task_id: String,
    pub name: String,
    pub partial_args: String,
    pub partial_args_truncated: bool,
    pub finished: bool,
    pub delta_queue: VecDeque<String>,
    pub last_activity: Instant,
}

pub struct DeltaAccumulator {
    state: Arc<Mutex<HashMap<ToolCallId, ToolState>>>,
    peer_events: Option<mpsc::Sender<PeerDeltaEvent>>,
    memory_watermark_bytes: usize,
}

impl DeltaAccumulator {
    pub fn new(memory_watermark_bytes: usize) -> Self {
        Self::new_with_peer_events(memory_watermark_bytes, None)
    }

    pub fn new_with_peer_events(
        memory_watermark_bytes: usize,
        peer_events: Option<mpsc::Sender<PeerDeltaEvent>>,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(HashMap::new())),
            peer_events,
            memory_watermark_bytes,
        }
    }

    pub fn start_tool(
        &self,
        id: ToolCallId,
        task_id: String,
        name: String,
    ) -> Result<(), AccumulationError> {
        {
            let mut map = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if map.contains_key(&id) {
                return Err(AccumulationError::DuplicateId(id));
            }

            if self.estimate_memory_usage(&map) > self.memory_watermark_bytes {
                self.drop_oldest_pending(&mut map);
                // Second check: if still over watermark after eviction (e.g. all
                // entries are finished and nothing was removed), surface the error
                // so callers can observe memory pressure rather than silently losing
                // the new entry.
                if self.estimate_memory_usage(&map) > self.memory_watermark_bytes {
                    return Err(AccumulationError::MemoryPressure);
                }
            }

            map.insert(
                id.clone(),
                ToolState {
                    id: id.clone(),
                    task_id: task_id.clone(),
                    name: name.clone(),
                    partial_args: String::new(),
                    partial_args_truncated: false,
                    finished: false,
                    delta_queue: VecDeque::new(),
                    last_activity: Instant::now(),
                },
            );
        }

        self.publish_event(PeerDeltaEvent::ToolStart {
            tool_call_id: id,
            task_id,
            name,
        });

        Ok(())
    }

    pub fn accumulate(&self, id: &ToolCallId, partial_json: &str) -> Result<(), AccumulationError> {
        let (task_id, truncation_event) = {
            let mut map = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let mut truncation_event = None;

            {
                let entry = map
                    .get_mut(id)
                    .ok_or_else(|| AccumulationError::UnknownId(id.clone()))?;

                if !is_valid_json_prefix(&entry.partial_args, partial_json) {
                    return Err(AccumulationError::MalformedPartial);
                }

                entry.last_activity = Instant::now();
                if entry.delta_queue.len() == MAX_TOOL_DELTA_QUEUE_DEPTH {
                    entry.delta_queue.pop_front();
                }
                entry.delta_queue.push_back(partial_json.to_string());

                for ch in partial_json.chars() {
                    let next_len = entry.partial_args.len().saturating_add(ch.len_utf8());
                    if next_len > MAX_STORED_PARTIAL_BYTES {
                        if !entry.partial_args_truncated {
                            entry.partial_args_truncated = true;
                            truncation_event = Some(PeerDeltaEvent::PartialArgsTruncated {
                                tool_call_id: id.clone(),
                                task_id: entry.task_id.clone(),
                                stored_bytes: MAX_STORED_PARTIAL_BYTES,
                            });
                        }
                        break;
                    }
                    entry.partial_args.push(ch);
                }
            }

            if self.estimate_memory_usage(&map) > self.memory_watermark_bytes {
                self.drop_oldest_pending(&mut map);
            }

            (
                map.get(id)
                    .map(|state| state.task_id.clone())
                    .unwrap_or_default(),
                truncation_event,
            )
        };

        self.publish_event(PeerDeltaEvent::ToolDelta {
            tool_call_id: id.clone(),
            task_id,
            partial_json: partial_json.to_string(),
        });
        if let Some(event) = truncation_event {
            self.publish_event(event);
        }

        Ok(())
    }

    pub fn finish(&self, id: &ToolCallId) {
        let task_id = {
            let mut map = self.state.lock().unwrap_or_else(|e| e.into_inner());
            map.get_mut(id).map(|entry| {
                entry.finished = true;
                entry.last_activity = Instant::now();
                entry.task_id.clone()
            })
        };

        if let Some(task_id) = task_id {
            self.publish_event(PeerDeltaEvent::ToolFinish {
                tool_call_id: id.clone(),
                task_id,
            });
        }
    }

    pub fn snapshot(&self) -> Arc<Mutex<HashMap<ToolCallId, ToolState>>> {
        Arc::clone(&self.state)
    }

    pub fn cleanup_finished(&self, older_than: Duration) -> usize {
        let mut map = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let cutoff = Instant::now() - older_than;
        let before = map.len();
        map.retain(|_, state| !state.finished || state.last_activity > cutoff);
        before.saturating_sub(map.len())
    }

    pub fn cleanup_task(&self, task_id: &str) -> usize {
        let mut map = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let before = map.len();
        map.retain(|_, state| state.task_id != task_id);
        before.saturating_sub(map.len())
    }

    fn estimate_memory_usage(&self, map: &HashMap<ToolCallId, ToolState>) -> usize {
        map.iter()
            .map(|(id, state)| {
                id.len()
                    + state.task_id.len()
                    + state.name.len()
                    + state.partial_args.len()
                    + state
                        .delta_queue
                        .iter()
                        .map(|delta| delta.len())
                        .sum::<usize>()
                    + 128
            })
            .sum()
    }

    fn drop_oldest_pending(&self, map: &mut HashMap<ToolCallId, ToolState>) {
        let Some(oldest_id) = map
            .iter()
            .filter(|(_, state)| !state.finished)
            .min_by_key(|(_, state)| state.last_activity)
            .map(|(id, _)| id.clone())
        else {
            return;
        };

        map.remove(&oldest_id);
    }

    fn publish_event(&self, event: PeerDeltaEvent) {
        let Some(peer_events) = &self.peer_events else {
            return;
        };

        if let Err(error) = peer_events.try_send(event) {
            tracing::debug!(%error, "dropped delta accumulator peer event");
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AccumulationError {
    #[error("unknown tool call id: {0}")]
    UnknownId(ToolCallId),
    #[error("duplicate tool call id: {0}")]
    DuplicateId(ToolCallId),
    #[error("malformed partial json")]
    MalformedPartial,
    /// Returned when the memory watermark is exceeded and no eligible pending
    /// entry can be evicted (for example, all in-flight entries are already
    /// finished). Callers should treat this as a best-effort degradation signal;
    /// the accumulator will not panic and the caller may continue without the
    /// new entry.
    #[error("memory watermark exceeded")]
    MemoryPressure,
}

fn is_valid_json_prefix(existing: &str, incoming: &str) -> bool {
    let mut brace_depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;

    for ch in existing.chars().chain(incoming.chars()) {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }

            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => brace_depth += 1,
            '}' => {
                brace_depth -= 1;
                if brace_depth < 0 {
                    return false;
                }
            }
            '[' => bracket_depth += 1,
            ']' => {
                bracket_depth -= 1;
                if bracket_depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_accumulate_finish_tracks_state_and_peer_events() {
        let (tx, mut rx) = mpsc::channel(8);
        let accumulator = DeltaAccumulator::new_with_peer_events(1_024, Some(tx));
        let tool_call_id = "tx_1_abcd".to_string();

        accumulator
            .start_tool(
                tool_call_id.clone(),
                "task-1".to_string(),
                "read_file".to_string(),
            )
            .unwrap();
        accumulator
            .accumulate(&tool_call_id, r#"{"path":"src/"#)
            .unwrap();
        accumulator
            .accumulate(&tool_call_id, r#"main.rs"}"#)
            .unwrap();
        accumulator.finish(&tool_call_id);

        let snapshot = accumulator.snapshot();
        let map = snapshot.lock().unwrap_or_else(|e| e.into_inner());
        let state = map.get(&tool_call_id).expect("tool state present");
        assert_eq!(state.partial_args, r#"{"path":"src/main.rs"}"#);
        assert!(!state.partial_args_truncated);
        assert_eq!(state.delta_queue.len(), 2);
        assert!(state.finished);

        assert_eq!(
            rx.blocking_recv().unwrap(),
            PeerDeltaEvent::ToolStart {
                tool_call_id: tool_call_id.clone(),
                task_id: "task-1".to_string(),
                name: "read_file".to_string(),
            }
        );
        assert_eq!(
            rx.blocking_recv().unwrap(),
            PeerDeltaEvent::ToolDelta {
                tool_call_id: tool_call_id.clone(),
                task_id: "task-1".to_string(),
                partial_json: r#"{"path":"src/"#.to_string(),
            }
        );
        assert_eq!(
            rx.blocking_recv().unwrap(),
            PeerDeltaEvent::ToolDelta {
                tool_call_id: tool_call_id.clone(),
                task_id: "task-1".to_string(),
                partial_json: r#"main.rs"}"#.to_string(),
            }
        );
        assert_eq!(
            rx.blocking_recv().unwrap(),
            PeerDeltaEvent::ToolFinish {
                tool_call_id,
                task_id: "task-1".to_string(),
            }
        );
    }

    #[test]
    fn partial_args_truncation_emits_peer_event_and_sets_flag() {
        let (tx, mut rx) = mpsc::channel(8);
        let accumulator = DeltaAccumulator::new_with_peer_events(512 * 1_024, Some(tx));
        let tool_call_id = "tx_3_trunc".to_string();
        accumulator
            .start_tool(
                tool_call_id.clone(),
                "task-trunc".to_string(),
                "write_file".to_string(),
            )
            .unwrap();

        let oversized = format!(
            "{{\"content\":\"{}\"}}",
            "x".repeat(MAX_STORED_PARTIAL_BYTES + 32)
        );
        accumulator.accumulate(&tool_call_id, &oversized).unwrap();

        let snapshot = accumulator.snapshot();
        let map = snapshot.lock().unwrap_or_else(|e| e.into_inner());
        let state = map.get(&tool_call_id).expect("tool state present");
        assert!(state.partial_args_truncated);
        assert_eq!(state.partial_args.len(), MAX_STORED_PARTIAL_BYTES);

        assert!(matches!(
            rx.blocking_recv().unwrap(),
            PeerDeltaEvent::ToolStart { .. }
        ));
        assert!(matches!(
            rx.blocking_recv().unwrap(),
            PeerDeltaEvent::ToolDelta { .. }
        ));
        assert_eq!(
            rx.blocking_recv().unwrap(),
            PeerDeltaEvent::PartialArgsTruncated {
                tool_call_id,
                task_id: "task-trunc".to_string(),
                stored_bytes: MAX_STORED_PARTIAL_BYTES,
            }
        );
    }

    #[test]
    fn rejects_malformed_partial_json() {
        let accumulator = DeltaAccumulator::new(1_024);
        let tool_call_id = "tx_2_dcba".to_string();
        accumulator
            .start_tool(
                tool_call_id.clone(),
                "task-2".to_string(),
                "apply_patch".to_string(),
            )
            .unwrap();

        let error = accumulator.accumulate(&tool_call_id, "}").unwrap_err();
        assert_eq!(error, AccumulationError::MalformedPartial);
    }

    #[test]
    fn memory_watermark_evicts_oldest_pending_tool() {
        let accumulator = DeltaAccumulator::new(1);

        accumulator
            .start_tool(
                "tx_1_aaaa".to_string(),
                "task-3".to_string(),
                "read_file".to_string(),
            )
            .unwrap();
        accumulator
            .start_tool(
                "tx_2_bbbb".to_string(),
                "task-3".to_string(),
                "apply_patch".to_string(),
            )
            .unwrap();

        let snapshot = accumulator.snapshot();
        let map = snapshot.lock().unwrap_or_else(|e| e.into_inner());
        assert!(!map.contains_key("tx_1_aaaa"));
        assert!(map.contains_key("tx_2_bbbb"));
    }

    #[test]
    fn memory_pressure_returned_when_no_pending_entry_can_be_evicted() {
        // A 1-byte watermark ensures any single entry exceeds the limit.
        let accumulator = DeltaAccumulator::new(1);

        // Insert and immediately finish one entry so it becomes ineligible for
        // eviction (drop_oldest_pending skips finished entries).
        let id = "tx_1_aaaa".to_string();
        accumulator
            .start_tool(id.clone(), "task-wp".to_string(), "read_file".to_string())
            .unwrap();
        accumulator.finish(&id);

        // A second start must observe MemoryPressure: the watermark is still
        // exceeded after the eviction pass because no pending entry exists.
        let result = accumulator.start_tool(
            "tx_2_bbbb".to_string(),
            "task-wp".to_string(),
            "apply_patch".to_string(),
        );
        assert_eq!(result.unwrap_err(), AccumulationError::MemoryPressure);
    }
}
