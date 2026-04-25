# ADR-020: Looping Architecture and Enriched Response Correctness

**Status:** Accepted  

## Decision

- Multi-tool round correctness: all tool results collected before the next model pulse; no partial-round routing.
- `ToolStatus::Error` is a first-class result type; model receives structured error, not a panic or silent drop.
- `enrich_tool_result` helper provides structured, per-call context at the L7 layer.

## References

- [`serde_json`](https://docs.rs/serde_json) — tool result serialization
