# Architecture Decision Records

ADR files follow the format: `ADR-XXX-<slug>.md`. Records under active maintenance live in `adr/`. Implementation-complete ADRs may remain in `adr/` until archival is finished; archived completed records live in `adr/completed/`.

Full implementation roadmap: `TASKS/ACTIVE-ROADMAP.md`.

## Status Vocabulary

| Status | Meaning |
| :--- | :--- |
| **Proposed** | Under discussion; implementation may be in progress |
| **Active** | Implementation in progress; subsequent phases or verification remain |
| **Accepted** | In effect — code must conform |
| **Locked** | Accepted and immutable — requires a new ADR to amend |
| **Complete** | All scoped items merged; pending housekeeping move to `completed/` |
| **Amended** | Base decision unchanged; specific items updated by an amendment ADR |
| **Deprecated by ADR-XXX** | Replaced; retained as a deprecated design record |

## Open ADRs

| ADR | Title | Status |
| :--- | :--- | :--- |
| [ADR-021](ADR-021-codebase-audit-unused-code-duplication-shared-code-opportunities.md) | Codebase audit — unused code | Accepted |
| [ADR-022](ADR-022-free-open-coding-agent-roadmap.md) | Free/open coding agent roadmap | Proposed (Phases G–H pending) |
| [ADR-022 amend 03-03](ADR-022-amendment-2026-03-03.md) | Command-execution scope | Amended |
| [ADR-022 amend 03-13](ADR-022-amendment-2026-03-13.md) | Capability-based approval | Amended |
| [ADR-022 amend 04-20](ADR-022-amendment-2026-04-20.md) | Phase G–H gate | Amended |
| [ADR-023](ADR-023-deterministic-edit-loop.md) | Deterministic edit loop | Locked |
| [ADR-024](ADR-024-zero-licensing-cost-agent-parity-gaps.md) | Zero-licensing-cost parity gaps | Proposed (PG-03 pending) |
| [ADR-028](ADR-028-application-facade-and-transport-boundaries.md) | Application facade and transport boundaries | Active |
| [ADR-029](ADR-029-stream-parser-completeness-and-session-persistence.md) | Stream parser completeness | Accepted |
| [ADR-030](ADR-030-runtime-task-state-and-orchestrator-control-flow.md) | Runtime task state and orchestrator control flow | Accepted |
| [ADR-031](ADR-031-operator-surface-ui-overhaul.md) | Operator surface UI overhaul | Accepted |
| [ADR-032](ADR-032-prompt-area-interactivity-and-context-guard.md) | Prompt area interactivity and context guard | Accepted |
| [ADR-033](ADR-033-hybrid-retrieval-context-architecture.md) | Hybrid retrieval context architecture | Accepted |
| [ADR-034](ADR-034-multi-agent-parallel-task-execution.md) | Multi-agent parallel task execution | Active |
| [ADR-035](ADR-035-undo-checkpoints-and-binary-safe-rollback.md) | Undo checkpoints and binary-safe rollback | Accepted |
| [ADR-038](ADR-038-memory-first-architecture-with-minimal-disk-io.md) | Memory-first architecture with minimal disk I/O | Accepted |
| [ADR-038 amend 04-13](ADR-038-amendment-2026-04-13.md) | Task-state cold-start memory bounds | Amended |
| [ADR-039](ADR-039-neutral-cli-voice-and-spatial-status-language.md) | Neutral CLI voice and spatial status language | Proposed (Batches B–D pending) |
| [ADR-040](ADR-040-real-time-local-turn-telemetry.md) | Real-time local turn telemetry | Proposed |
| [ADR-041](ADR-041-transcript-renderer-wiring-and-compact-tool-paragraphs.md) | Transcript renderer and compact tool paragraphs | Accepted |
| [ADR-042](ADR-042-tool-registration-and-approval-layer.md) | Tool registration and approval layer | Accepted |
| [ADR-043](ADR-043-structured-output-parser-adoption-gates.md) | Structured output parser adoption gates | Active |
| [ADR-044](ADR-044-test-suite-scalability-and-fixture-patterns.md) | Test suite scalability and fixture patterns | Proposed |
| [ADR-045](ADR-045-replay-first-task-document-and-single-writer-state.md) | Replay-first task document and single-writer state | Proposed |
| [ADR-046](ADR-046-agent-peer-message-channel.md) | Agent peer message channel | Accepted |
| [ADR-047](ADR-047-block-delta-protocol-discovery-dual-protocol-support.md) | Block-delta protocol discovery and dual-protocol support | Accepted |
| [ADR-047 amend 04-16](ADR-047-amendment-2026-04-16.md) | API-first runtime event envelope | Amended |
| [ADR-047 amend 04-16 add](ADR-047-amendment-2026-04-16-addendum.md) | Envelope addendum | Amended |
| [ADR-047 amend 04-20](ADR-047-amendment-2026-04-20.md) | RuntimeEnvelope consumer boundary | Amended |
| [ADR-048](ADR-048-operator-permissions-overlay-and-mode-precedence.md) | Operator permissions overlay and mode precedence | Proposed |

## Top-Level ADRs Pending Archival

| ADR | Title | Status |
| :--- | :--- | :--- |
| [ADR-013](ADR-013-tui-completion-deployment-plan.md) | TUI completion and deployment plan | Accepted |
| [ADR-018](ADR-018-managed-tui-scrollback-streaming-cell-overlays.md) | Managed TUI scrollback | Deprecated by ADR-027 |
| [ADR-025](ADR-025-runtime-json-handoff-contract.md) | Runtime JSON handoff contract | Complete |
| [ADR-026](ADR-026-localapiserver-transport-binding.md) | LocalApiServer transport binding | Complete |
| [ADR-027](ADR-027-full-screen-tui-command-session-capture.md) | Full-screen TUI command-session capture | Accepted |

## Archived Completed ADR Records

| ADR | Title | Status |
| :--- | :--- | :--- |
| [ADR-001](completed/ADR-001-tdm-agentic-manifest-strategy.md) | TDM agentic manifest strategy | Accepted |
| [ADR-002](completed/ADR-002-lexical-path-normalization.md) | Lexical path normalization | Accepted |
| [ADR-003](completed/ADR-003-dual-protocol-api-auto-detection.md) | Dual-protocol API auto-detection | Accepted |
| [ADR-004](completed/ADR-004-runtime-seam-headless-first.md) | Runtime seam headless-first | Deprecated by ADR-006/007 |
| [ADR-005](completed/ADR-005-cfg-test-mock-injection.md) | `cfg(test)` mock injection | Accepted |
| [ADR-006](completed/ADR-006-runtime-mode-contracts.md) | Runtime mode contracts | Accepted |
| [ADR-007](completed/ADR-007-runtime-accepted-dispatch-no-alt-routing.md) | Accepted dispatch no-alt-routing | Accepted |
| [ADR-008](completed/ADR-008-runtime-cutover-parity-guardrails.md) | Runtime cutover parity guardrails | Accepted |
| [ADR-009](completed/ADR-009-runtime-core-tui-interaction-contract.md) | TUI interaction contract | Accepted |
| [ADR-010](completed/ADR-010-runtime-core-tui-viewport-and-transcript.md) | TUI viewport and transcript | Accepted |
| [ADR-011](completed/ADR-011-runtime-core-tui-render-loop-and-lifecycle.md) | TUI render loop and lifecycle | Accepted |
| [ADR-012](completed/ADR-012-runtime-core-tui-deployment-gate.md) | TUI deployment gate | Accepted |
| [ADR-013](completed/ADR-013-tui-completion-deployment-plan.md) | TUI completion and deployment plan | Accepted |
| [ADR-014](completed/ADR-014-runtime-core-policy-dedup-and-enforcement.md) | Policy dedup and enforcement | Accepted |
| [ADR-015](completed/ADR-015-local-endpoint-text-protocol-default.md) | Local endpoint text-protocol default | Accepted |
| [ADR-016](completed/ADR-016-local-tool-loop-guard-and-correction.md) | Local tool-loop guard and correction | Accepted |
| [ADR-017](completed/ADR-017-append-single-session-runtime.md) | Append single-session runtime | Deprecated by ADR-018 |
| [ADR-018](completed/ADR-018-managed-tui-scrollback-streaming-cell-overlays.md) | Managed TUI scrollback | Deprecated by ADR-027 |
| [ADR-019](completed/ADR-019-adr-018-follow-up-correctness-cutover-cleanup.md) | ADR-018 correctness cutover | Deprecated by ADR-027 |
| [ADR-020](completed/ADR-020-looping-architecture-enriched-response-correctness.md) | Looping architecture enriched response | Accepted |
| [ADR-025](completed/ADR-025-runtime-json-handoff-contract.md) | Runtime JSON handoff contract | Complete |
| [ADR-026](completed/ADR-026-localapiserver-transport-binding.md) | LocalApiServer transport binding | Complete |
| [ADR-027](completed/ADR-027-full-screen-tui-command-session-capture.md) | Full-screen TUI command-session capture | Accepted |
