# ADR-025 Phase I Continuation: PI-10, then PI-12

## Status update

This batch is now implemented in the current tree after the PR #101 follow-up.
The next dependency-sequenced batch is ADR-026 Phase I transport binding (`PI-13` and `PI-14` in parallel) after ADR-024 reconciliation.

## Context

The initial validation passed on 2026-03-15 and is recorded in
`adr/ADR-022-free-open-coding-agent-roadmap.md`. The ADR-025 Phase I kickoff
(PI-09 and PI-11) merged in PR #99 and established the accepted runtime
handoff types, normative tool-call grammar, and versioned schema assets.

The next work batch is the ADR-025 Phase I continuation. The dependency
order is:

- PI-10 immediately after PI-09 (normalization layer)
- PI-12 after both PI-10 and PI-11 (test coverage)

ADR-026 remains sequenced after ADR-025 closeout and ADR-024 reconciliation.
ADR-028 is the boundary ADR that post-gate implementation must respect.
ADR-029 remains active follow-up work but does not supersede ADR-025 order.
ADR-030 defines the task-state and orchestrator control-flow contract that
downstream runtime work must preserve.

## Required source documents

Read these before starting implementation:

### Repo-local ADR and task sources

- `AGENTS.md`
- `CONTRIBUTING.md`
- `TASKS/ACTIVE-ROADMAP.md`
- `TASKS/TASKS-WORK-MAP.md`
- `adr/ADR-022-free-open-coding-agent-roadmap.md`
- `adr/ADR-025-runtime-json-handoff-contract.md`
- `adr/ADR-026-localapiserver-transport-binding.md`
- `adr/ADR-028-application-facade-and-transport-boundaries.md`
- `adr/ADR-029-stream-parser-completeness-and-session-persistence.md`
- `adr/ADR-030-runtime-task-state-and-orchestrator-control-flow.md`

### Private skill sources

If running locally, load these from:

- `../vexdraft/.agents/skills/vex-local-bash/SKILL.md`
- `../vexdraft/.agents/skills/vex-remote-contract/SKILL.md`
- `../vexdraft/.agents/skills/vex-rust-arch/SKILL.md`

If running on GitHub.com, resolve the same files from:

- <https://github.com/aistar-au/vexdraft/blob/main/.agents/skills/vex-local-bash/SKILL.md>
- <https://github.com/aistar-au/vexdraft/blob/main/.agents/skills/vex-remote-contract/SKILL.md>
- <https://github.com/aistar-au/vexdraft/blob/main/.agents/skills/vex-rust-arch/SKILL.md>

## Batch items

### PI-10 — Normalization layer (start first)

Add normalization layer from provider/native stream updates into accepted
runtime envelopes.

Scope defined in `adr/ADR-025-runtime-json-handoff-contract.md`:

- Runtime injects `ToolCall.id` in the format `call_<utc-ms>_<4-hex-random>`.
  Provider IDs are discarded at the normalization boundary.
- Normalization mappings:
  - `ContentBlock::ToolUse { id, name, input }` to `ToolCall { id, name, arguments }`
    (runtime discards provider id)
  - `ContentBlock::ToolResult { tool_use_id, content, is_error }` to
    `ToolResult { tool_call_id, tool_name, is_error, output }`
  - `StreamBlock::ToolCall { id, name, input }` to `ToolCall { id, name, arguments }`
    (runtime generates new id)
  - `StreamBlock::ToolResult { tool_call_id, output, is_error }` to
    `ToolResult { tool_call_id, tool_name, is_error, output }`
  - `UiUpdate::StreamDelta(text)` to `AssistantDelta { text }`
  - `UiUpdate::PulseComplete` to `AssistantMessage { content }` then
    `PulseEnd { status: "completed", ... }`
  - `UiUpdate::Error(message)` to `Error { code, message, recoverable }` then
    `PulseEnd { status: "failed", ... }`
  - `UiUpdate::ToolApprovalRequest(req)` to
    `ApprovalRequest { capability, scope, tool_name }`
  - `RuntimeRequest::ApproveCapability` to
    `ApprovalResolved { capability, scope, approved: true }`
  - `RuntimeRequest::DenyCapability` to
    `ApprovalResolved { capability, scope, approved: false }`
  - `UiUpdate::StreamBlockStart/Delta/Complete` are not projected
    (TUI render bookkeeping only)
  - Grammar `tool_call_array` produces one `ToolCall` envelope per array
    element with runtime-generated `id` and `seq`
- `AssistantMessage` assembly: `PulseComplete` is the normative source; deltas
  are accumulated, a final `AssistantMessage` is emitted immediately before
  `PulseEnd`, and `BatchMode` derives `TurnRecord.response` from that content.
- `ToolResult.tool_name` remains `Option<String>` until ADR-024 PF-01/PF-02
  (McpRegistry and approval wiring) are complete.
- `StreamBlockStart/Delta/Complete` explicit no-project rule: these are TUI
  render bookkeeping only and must not appear in the shared envelope stream.

Target files (expected):

- `src/runtime/json_handoff.rs` (extend with normalization functions)
- `src/runtime.rs` (re-exports if needed)

### PI-12 — Serde round-trip, schema parity, grammar parity, and BatchMode derivation tests (after PI-10)

Add comprehensive test coverage for the ADR-025 handoff contract.

Scope defined in `adr/ADR-025-runtime-json-handoff-contract.md`:

- Serde round-trip tests for all envelope and request types
- Schema parity tests (CI verifies schema-generation parity and round-trip
  stability)
- Grammar parity tests (GBNF grammar consistency with schema)
- BatchMode derivation tests (replaying shared envelopes reconstructs the
  existing summarized JSONL shape)
- Required assertions:
  - First envelope of every pulse has `seq == 1`
  - `TurnRecord` + `SummaryRecord` replay from shared envelopes matches the
    existing JSONL shape modulo JSON field ordering
  - `TurnRecord.response` uses `AssistantMessage.content` when present and
    falls back to concatenated `AssistantDelta.text`
  - `TurnRecord.changed_files` matches `turn_end.changed_files`
  - `SummaryRecord.status` matches final `turn_end.status`
  - Recoverable vs non-recoverable `error` envelopes follow ordering rules
  - `MaxTurnsReached` is always followed by `PulseEnd { status: "failed" }`

PI-12 depends on both PI-10 and PI-11. Do not start PI-12 until PI-10 is
complete.

## Dependency order

```text
PI-09 (complete) + PI-11 (complete)
  -> PI-10 (normalization layer — start now)
  -> PI-12 (test coverage — after PI-10)
  -> ADR-026 PI-13 + PI-14 (parallel, after PI-12 and ADR-024 reconciliation)
  -> PI-15 (after PI-13 and PI-14)
  -> PI-16 (last)
```

## Constraints

- Treat the first-release gate as already passed. Do not re-run validation gate work.
- Do not re-open ADR-022 validation except to cite it as completed dependency.
- Keep ADR-026 sequenced after ADR-025 closeout and ADR-024 reconciliation.
- Treat ADR-028 as boundary ADR for post-gate implementation.
- Treat ADR-029 as active follow-up work; do not let it supersede ADR-025 order.
- Treat ADR-030 as the control-flow ADR for post-gate runtime work; do not let
  it supersede ADR-025 dependency order.
- Follow all no-touch / explicit-approval / exact-diff rules from AGENTS.md,
  CONTRIBUTING.md, and the private local skills.

## Verification baseline

- `cargo test --all-targets`
- `make gate-fast`
- `bash scripts/check_no_alternate_routing.sh`
- `bash scripts/check_forbidden_imports.sh`
- `bash scripts/check_forbidden_names.sh`

Add targeted ADR-025 tests as part of PI-12. Run verification after each item.

## PR motivation framing

- The initial validation has already passed and is recorded in ADR-022.
- With this branch, roadmap and work map advance to ADR-026 Phase I
  transport binding (`PI-13` and `PI-14` in parallel) after ADR-024
  reconciliation.
- This batch continues the post-gate Phase I track in documented dependency
  order.
- PI-10 adds the normalization layer that maps provider/native stream updates
  into shared runtime envelopes.
- PI-12 adds serde round-trip, schema parity, grammar parity, and BatchMode
  derivation test coverage.
- ADR-026, ADR-028, ADR-029, and ADR-030 remain active but dependency-sequenced
  follow-up ADRs.

## Expected deliverables

- PI-10 implementation: normalization functions in `src/runtime/json_handoff.rs`
  mapping provider-native types to shared envelopes
- PI-12 test coverage: round-trip, schema parity, grammar parity, and BatchMode
  derivation tests
- Evidence blocks in `adr/ADR-025-runtime-json-handoff-contract.md` for PI-10
  and PI-12
- Mark PI-10 and PI-12 complete in ADR-025 checklist
- Update `TASKS/ACTIVE-ROADMAP.md` and `TASKS/TASKS-WORK-MAP.md` to point
  to the next dependency-correct batch (ADR-026 Phase I)
- Update `TASKS/completed/REPO-RAW-URL-MAP.md` if tracked files change
- Clean handoff for the next dependency-sequenced batch (ADR-026 PI-13 + PI-14)
