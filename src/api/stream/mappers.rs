//! Protocol mappers for tool-call delta serialisation.
//!
//! Thin stateless serialisers that convert [`ToolState`] snapshots from the
//! [`DeltaAccumulator`] into SSE frames for either the Block-Delta or
//! Choices-Delta wire format.  The mapper is selected once per connection from
//! [`crate::config::ApiClientConfig::explicit_protocol`] (or the default
//! `ProtocolVariant::BlockDelta`) and kept for the lifetime of the task.
//!
//! ADR-047 Phase 1.

use crate::config::ProtocolVariant;
use crate::runtime::delta_accumulator::ToolState;

/// A complete SSE event: `data: <payload>\n\n`.
///
/// The inner string is the full text ready for transmission over the SSE
/// channel; callers must not add a second `data:` prefix.
pub struct SseFrame(pub String);

fn sse(payload: String) -> SseFrame {
    SseFrame(format!("data: {}\n\n", payload))
}

/// Thin stateless serialiser over [`ToolState`] snapshots.
///
/// Implementations are required to be `Send + Sync + 'static` so they can be
/// stored in Axum state or passed across task boundaries.
pub trait ProtocolMapper: Send + Sync + 'static {
    /// Emit the start-of-tool frame for the given state.
    fn tool_start_frame(&self, index: usize, state: &ToolState) -> SseFrame;
    /// Emit a single partial-JSON delta frame.
    fn tool_delta_frame(&self, index: usize, partial_json: &str) -> SseFrame;
    /// Emit the end-of-tool frame.
    fn tool_finish_frame(&self, index: usize) -> SseFrame;

    /// Drain all queued delta chunks from `state` and return frames in order.
    ///
    /// Convenience wrapper over [`ProtocolMapper::tool_delta_frame`] that
    /// iterates `state.delta_queue` without consuming it.
    fn drain_tool_deltas(&self, index: usize, state: &ToolState) -> Vec<SseFrame> {
        state
            .delta_queue
            .iter()
            .map(|delta| self.tool_delta_frame(index, delta))
            .collect()
    }
}

/// Serialises tool calls as Block-Delta SSE events (ADR-047 default format).
///
/// Emits `content_block_start` → N × `content_block_delta` → `content_block_stop`.
pub struct BlockDeltaMapper;

impl ProtocolMapper for BlockDeltaMapper {
    fn tool_start_frame(&self, index: usize, state: &ToolState) -> SseFrame {
        sse(serde_json::json!({
            "type": "content_block_start",
            "index": index,
            "content_block": {
                "type": "tool_use",
                "id": &state.id,
                "name": &state.name,
                "input": {}
            }
        })
        .to_string())
    }

    fn tool_delta_frame(&self, index: usize, partial_json: &str) -> SseFrame {
        sse(serde_json::json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {
                "type": "input_json_delta",
                "partial_json": partial_json
            }
        })
        .to_string())
    }

    fn tool_finish_frame(&self, index: usize) -> SseFrame {
        sse(serde_json::json!({
            "type": "content_block_stop",
            "index": index
        })
        .to_string())
    }
}

/// Serialises tool calls as Choices-Delta SSE events.
///
/// Produces frames compatible with the `chat/completions` streaming format:
/// `choices[].delta.tool_calls[]` with partial argument strings.
pub struct ChoicesDeltaMapper;

impl ProtocolMapper for ChoicesDeltaMapper {
    fn tool_start_frame(&self, index: usize, state: &ToolState) -> SseFrame {
        sse(serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": index,
                        "id": &state.id,
                        "type": "function",
                        "function": {
                            "name": &state.name,
                            "arguments": ""
                        }
                    }]
                },
                "finish_reason": null
            }]
        })
        .to_string())
    }

    fn tool_delta_frame(&self, index: usize, partial_json: &str) -> SseFrame {
        sse(serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": index,
                        "function": {
                            "arguments": partial_json
                        }
                    }]
                },
                "finish_reason": null
            }]
        })
        .to_string())
    }

    fn tool_finish_frame(&self, index: usize) -> SseFrame {
        sse(serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "tool_calls"
            }]
        })
        .to_string())
    }
}

/// Return a boxed mapper for the given protocol variant.
///
/// Called once per request by the `turns_handler` to avoid embedding
/// protocol-selection logic in the handler itself.
pub fn mapper_for_variant(variant: ProtocolVariant) -> Box<dyn ProtocolMapper> {
    match variant {
        ProtocolVariant::BlockDelta => Box::new(BlockDeltaMapper),
        ProtocolVariant::ChoicesDelta => Box::new(ChoicesDeltaMapper),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::delta_accumulator::{AccumulationError, DeltaAccumulator, PeerDeltaEvent};
    use crate::runtime::tokio::sync::mpsc;
    use std::collections::VecDeque;
    use std::time::Instant;

    fn make_tool_state(id: &str, name: &str) -> ToolState {
        ToolState {
            id: id.to_string(),
            task_id: "task-1".to_string(),
            name: name.to_string(),
            partial_args: String::new(),
            finished: false,
            delta_queue: VecDeque::new(),
            last_activity: Instant::now(),
        }
    }

    fn parse_frame(frame: &SseFrame) -> serde_json::Value {
        let payload = frame
            .0
            .strip_prefix("data: ")
            .expect("missing data: prefix")
            .trim_end_matches('\n');
        serde_json::from_str(payload).expect("frame payload is not valid JSON")
    }

    /// 1. Both mappers encode the same id/name and use their respective formats.
    #[test]
    fn dual_protocol_parity() {
        let state = make_tool_state("tx_1_abcd", "read_file");
        let block = BlockDeltaMapper;
        let choices = ChoicesDeltaMapper;

        let b = parse_frame(&block.tool_start_frame(0, &state));
        let c = parse_frame(&choices.tool_start_frame(0, &state));

        assert_eq!(b["type"], "content_block_start");
        assert_eq!(b["content_block"]["id"], "tx_1_abcd");
        assert_eq!(b["content_block"]["name"], "read_file");

        assert!(c["choices"].is_array());
        let tc = &c["choices"][0]["delta"]["tool_calls"][0];
        assert_eq!(tc["id"], "tx_1_abcd");
        assert_eq!(tc["function"]["name"], "read_file");
    }

    /// 2. Multiple tools at distinct indices do not share index values.
    #[test]
    fn interleaved_text_and_tool_deltas() {
        let s0 = make_tool_state("tx_0_aaaa", "write_file");
        let s1 = make_tool_state("tx_1_bbbb", "read_file");
        let mapper = BlockDeltaMapper;

        let f0 = parse_frame(&mapper.tool_start_frame(0, &s0));
        let f1 = parse_frame(&mapper.tool_start_frame(1, &s1));
        let d0 = parse_frame(&mapper.tool_delta_frame(0, r#"{"path":"a.rs""#));
        let d1 = parse_frame(&mapper.tool_delta_frame(1, r#"{"path":"b.rs""#));

        assert_eq!(f0["index"], 0);
        assert_eq!(f1["index"], 1);
        assert_eq!(d0["index"], 0);
        assert_eq!(d1["index"], 1);
        assert_ne!(f0["content_block"]["id"], f1["content_block"]["id"]);
    }

    /// 3. Truncated (partial) JSON in a delta does not panic and serialises as-is.
    #[test]
    fn truncated_stream_recovery() {
        let mapper = BlockDeltaMapper;
        let frame = parse_frame(&mapper.tool_delta_frame(0, r#"{"path":"src/m"#));
        assert_eq!(frame["type"], "content_block_delta");
        assert_eq!(frame["delta"]["type"], "input_json_delta");
        let pj = frame["delta"]["partial_json"].as_str().unwrap();
        assert!(pj.starts_with('{'), "partial_json should start with open brace");
    }

    /// 4. Accumulator rejects partial JSON that would unbalance brace depth.
    #[test]
    fn malformed_partial_rejection() {
        let acc = DeltaAccumulator::new(1024 * 1024);
        let id = "tx_1_abcd".to_string();
        acc.start_tool(id.clone(), "task-1".to_string(), "read_file".to_string())
            .unwrap();
        acc.accumulate(&id, r#"{"path":"#).unwrap();
        // Closing braces that exceed the open brace count
        let result = acc.accumulate(&id, "}}}");
        assert_eq!(result, Err(AccumulationError::MalformedPartial));
    }

    /// 5. PeerDeltaEvents are propagated for every accumulator mutation.
    #[test]
    fn peer_channel_propagation() {
        let (tx, mut rx) = mpsc::channel(8);
        let acc = DeltaAccumulator::new_with_peer_events(1024 * 1024, Some(tx));
        let id = "tx_2_cccc".to_string();
        acc.start_tool(id.clone(), "task-1".to_string(), "list_files".to_string())
            .unwrap();
        acc.accumulate(&id, r#"{"dir":"#).unwrap();
        acc.finish(&id);

        assert!(matches!(rx.try_recv().unwrap(), PeerDeltaEvent::ToolStart { .. }));
        assert!(matches!(
            rx.try_recv().unwrap(),
            PeerDeltaEvent::ToolDelta { .. }
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            PeerDeltaEvent::ToolFinish { .. }
        ));
    }

    /// 6. `AccumulationError::MemoryPressure` is returned when watermark is
    ///    exceeded and no pending (non-finished) entry can be evicted.
    #[test]
    fn memory_watermark_enforcement() {
        // Watermark of 1 byte ensures the first insert saturates it.
        let acc = DeltaAccumulator::new(1);
        let id = "tx_1_aaaa".to_string();
        acc.start_tool(id.clone(), "task-1".to_string(), "tool_a".to_string())
            .unwrap();
        // Finish it so drop_oldest_pending cannot evict it (only evicts !finished).
        acc.finish(&id);

        let result = acc.start_tool(
            "tx_2_bbbb".to_string(),
            "task-1".to_string(),
            "tool_b".to_string(),
        );
        assert_eq!(result, Err(AccumulationError::MemoryPressure));
    }

    /// 7. `cleanup_task` removes exactly the entries belonging to that task.
    #[test]
    fn task_cleanup_on_completion() {
        let acc = DeltaAccumulator::new(1024 * 1024);
        acc.start_tool("tx_1_aaaa".to_string(), "task-a".to_string(), "tool_1".to_string())
            .unwrap();
        acc.start_tool("tx_2_bbbb".to_string(), "task-b".to_string(), "tool_2".to_string())
            .unwrap();
        acc.start_tool("tx_3_cccc".to_string(), "task-a".to_string(), "tool_3".to_string())
            .unwrap();

        let removed = acc.cleanup_task("task-a");
        assert_eq!(removed, 2, "expected 2 entries removed for task-a");

        let snapshot = acc.snapshot();
        let map = snapshot.lock().unwrap();
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("tx_2_bbbb"), "task-b entry should remain");
    }
}
