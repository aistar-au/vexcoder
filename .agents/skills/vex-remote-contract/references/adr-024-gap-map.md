# ADR-024 Gap Map (Dispatch Reference)

Source of truth:
`TASKS/ADR-024-zero-licensing-cost-agent-parity-gaps.md`

This file separates normative gap semantics from inferred implementation touchpoints.

## Interpretation rules

- Normative statements come from ADR-024 text.
- Touchpoint paths are inference and must be source-verified before assertion.
- If source and inference diverge, source wins.

## Gap map

| Gap | Normative scope (abridged) | Likely touchpoints (inference) |
| :--- | :--- | :--- |
| 1 | OS-level sandboxing abstraction | `src/runtime/command.rs`, `src/runtime/policy.rs` |
| 2 | Non-interactive execution mode (`vex exec`) | `src/runtime/mode.rs`, `src/bin/vex.rs` |
| 3 | Layered configuration chain | `src/config.rs` |
| 4 | Project instructions file injection | `src/runtime/context.rs`, `src/runtime/context_assembler.rs` |
| 5/15/31 | MCP integration and management surface | `src/tools/operator.rs`, `src/runtime/approval.rs`, config parsing paths |
| 8 | Runtime model switching command surface | `src/app.rs`, `src/runtime/context.rs` |
| 13/14/18/22/23 | Slash-command control surface extensions | `src/app.rs`, `src/ui/render.rs` |
| 24 | Git workflow helper surfaces | `src/bin/vex.rs`, runtime command paths |
| 28/29/30/32 | Session usage/export/resume/print CLI behavior | `src/runtime/task_state.rs`, `src/bin/vex.rs` |
| 35 | Workspace exploration tools | `src/tools/operator.rs`, tool dispatch table |

## Dispatch note

When building dispatch prompts for ADR-024 work:

- quote the gap number and normative sentence from ADR-024.
- tag any path hint as `(inference)` until verified at the current commit SHA.
