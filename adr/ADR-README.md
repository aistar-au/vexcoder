# Architecture Decision Records

All ADR files are stored under `adr/`.

- Open ADRs: `adr/ADR-XXX-*.md`
- Accepted/superseded ADR records: `adr/completed/ADR-XXX-*.md`

## Status vocabulary

| Status | Meaning |
| :--- | :--- |
| **Proposed** | Under discussion and dispatchable |
| **Amended** | Existing ADR amended without superseding the base decision |
| **Active** | In progress in the current tree; subsequent phases or verification may still remain |
| **Accepted** | In effect -- code must conform |
| **Complete** | All scoped implementation items are complete; housekeeping move to `completed/` still remains |
| **Superseded by ADR-XXX** | Replaced; retained for history |
| **Locked** | Accepted and immutable -- no further amendments without a new ADR |
| **Deprecated** | Was accepted, no longer applies |

## Open ADRs (Dispatch)

| ADR | Title | Status | Remaining |
| :--- | :--- | :--- | :--- |
| [ADR-021](ADR-021-codebase-audit-unused-code-duplication-shared-code-opportunities.md) | Codebase audit (unused code) | Accepted | 0 items remaining; all P1/P2/P3 items complete |
| [ADR-022 amendment](ADR-022-amendment-2026-03-13.md) | Command-execution amendment | Amended | Amendment only |
| [ADR-022](ADR-022-free-open-coding-agent-roadmap.md) | Free/Open coding agent roadmap | Proposed (milestone-1 passed 2026-03-15) | Post-milestone phases G/H |
| [ADR-024](ADR-024-zero-licensing-cost-agent-parity-gaps.md) | Zero-licensing-cost agent parity gaps | Proposed (pre-milestone complete) | 1 external item in the next batch: PG-03 tap auto-dispatch after the tap repo exists |
| [ADR-028](ADR-028-application-facade-and-transport-boundaries.md) | Application facade and transport boundaries | Active | Phase 1+2 merged; boundary tests now cover grouped, multiline, and `super::`-relative imports |
| [ADR-029](ADR-029-stream-parser-completeness-and-session-persistence.md) | Stream parser completeness | Accepted | All 8 decision items verified (2026-03-28) |
| [ADR-030](ADR-030-runtime-task-state-and-orchestrator-control-flow.md) | Runtime task state and orchestrator control flow | Accepted | All 6 coverage requirements verified (2026-03-28) |
| [ADR-031](ADR-031-operator-surface-ui-overhaul.md) | Operator surface UI overhaul | Accepted (all batches A-E merged) | 0 items remaining; fullscreen auto-fit behavior documented |
| [ADR-032](ADR-032-prompt-area-interactivity-and-context-guard.md) | Prompt area interactivity and context guard | Accepted | 0 items remaining; item 9 transferred to ADR-033 |
| [ADR-033](ADR-033-hybrid-retrieval-context-architecture.md) | Hybrid retrieval context architecture | Accepted (all phases 1-4 merged) | 0 items remaining |
| [ADR-034](ADR-034-multi-agent-parallel-task-execution.md) | Multi-agent / parallel task execution | Active (Phase A + B-E merged) | Hardening merged for serialized concurrency caps, prompt guard, explicit release, and normalized watch/boundary coverage |
| [ADR-035](ADR-035-undo-checkpoints-and-binary-safe-rollback.md) | Undo checkpoints and binary-safe rollback | Accepted | 0 items remaining; Gap 14 rollback strategy formalized for `/undo` |
| [ADR-038](ADR-038-memory-first-architecture-with-minimal-disk-io.md) | Memory-first architecture with minimal disk I/O | Accepted (Batches D-H merged) | 0 items remaining |
| [ADR-039](ADR-039-neutral-cli-voice-and-spatial-status-language.md) | Neutral CLI voice and spatial status language | Proposed (Batch A merged on main) | Batch A merged in PR #292; search.exclude path-boundary fix in PR #293; remaining batches B-D cover vocabulary, active indicator, and paragraph progress stream |
| [ADR-040](ADR-040-real-time-local-turn-telemetry.md) | Real-time local turn telemetry | Proposed | Telemetry labels updated to arrow notation in ADR-041 |
| [ADR-041](ADR-041-transcript-renderer-wiring-and-compact-tool-paragraphs.md) | Transcript renderer wiring and compact tool paragraphs | Accepted | Normaliser flush, compact tool paragraphs, arrow telemetry labels |
| [ADR-043](ADR-043-structured-output-parser-adoption-gates.md) | Structured output parser adoption gates | Active, with open adoption gates | Present in tree but not the default runtime parser path; 3 gates: live wiring, parity, defect reduction |
| [ADR-044](ADR-044-test-suite-scalability-and-fixture-patterns.md) | Test suite scalability and fixture patterns | Proposed | 3-phase implementation roadmap; Phase 1: aggregator + RAII helpers; Phase 2: builder API + async; Phase 3: parameterization + coverage |
| [ADR-045](ADR-045-replay-first-task-document-and-single-writer-state.md) | Replay-first task document and single-writer state | Proposed | Defines `TaskDocumentCondenser` as sole writer, `RuntimeEventLog` as canonical persisted history, full event coverage requirement, full-fidelity checkpoints, and session rollback markers; supersedes lossy `persistable_snapshot` as canonical resume source |

## Implementation-Complete ADRs (pending move to completed/)

These ADRs have all implementation items merged but their files remain in
the top-level `adr/` directory pending a housekeeping move.

| ADR | Title | Status | Notes |
| :--- | :--- | :--- | :--- |
| [ADR-013](ADR-013-tui-completion-deployment-plan.md) | TUI completion and deployment plan | Accepted | All phases complete |
| [ADR-018](ADR-018-managed-tui-scrollback-streaming-cell-overlays.md) | Managed TUI scrollback | Superseded by ADR-027 | Retained for history |
| [ADR-023](ADR-023-deterministic-edit-loop.md) | Deterministic edit loop | Complete | EL-01 through EL-13 all merged |
| [ADR-025](ADR-025-runtime-json-handoff-contract.md) | Runtime JSON handoff contract | Complete | PI-09 through PI-12 all merged |
| [ADR-026](ADR-026-localapiserver-transport-binding.md) | LocalApiServer transport binding | Complete | PI-13 through PI-16 all merged |
| [ADR-027](ADR-027-full-screen-tui-command-session-capture.md) | Full-screen TUI command-session capture | Accepted (complete) | Supersedes ADR-018/019 |

## Remaining Work Summary (1 proposed in-tree ADR + 1 external dependency in the next batch)

ADR-039 now tracks the proposed operator-facing CLI voice lane: neutral spatial
vocabulary, status-copy normalization, semantic ANSI roles, and the
paragraph-oriented progress stream for long-running tasks. Batch A is
merged on main (PR #292): `Mapping adjacent sectors...`,
`State synchronized.`, and semantic color feedback now land on existing
surfaces. A subsequent fix in PR #293 normalizes `search.exclude` entries with
a trailing slash. Remaining work covers the broader vocabulary sweep, active
indicator, and paragraph-stream restructuring. ADR-038 is accepted and
complete. The only external prerequisite in the next batch remains ADR-024 PG-03 tap
auto-dispatch, which stays blocked until the separate tap repository exists.
Full history and ADR status detail are located in `TASKS/ACTIVE-ROADMAP.md`.

| Tier | Source | Items | Description |
| :--- | :--- | :--- | :--- |
| 11 | ADR-039 | 3 | Remaining batches: broader vocabulary pass, active indicator, paragraph progress stream |
| 8 | ADR-024 G/H + ADR-022 | 1 | Next batch planned external prerequisite: tap repository auto-dispatch |

## Completed ADR Records

| ADR | Title | Status |
| :--- | :--- | :--- |
| [ADR-001](completed/ADR-001-tdm-agentic-manifest-strategy.md) | Test-Driven Manifest (TDM) | Accepted |
| [ADR-002](completed/ADR-002-lexical-path-normalization.md) | Lexical path normalization | Accepted |
| [ADR-003](completed/ADR-003-dual-protocol-api-auto-detection.md) | Dual-protocol API auto-detection | Accepted |
| [ADR-004](completed/ADR-004-runtime-seam-headless-first.md) | Runtime seam headless-first | Superseded by ADR-006/007 |
| [ADR-005](completed/ADR-005-cfg-test-mock-injection.md) | cfg-test mock injection | Accepted |
| [ADR-006](completed/ADR-006-runtime-mode-contracts.md) | Runtime mode contracts | Accepted |
| [ADR-007](completed/ADR-007-runtime-canonical-dispatch-no-alt-routing.md) | Canonical dispatch no-alt-routing | Accepted |
| [ADR-008](completed/ADR-008-runtime-cutover-parity-guardrails.md) | Runtime cutover parity guardrails | Accepted |
| [ADR-009](completed/ADR-009-runtime-core-tui-interaction-contract.md) | TUI interaction contract | Accepted |
| [ADR-010](completed/ADR-010-runtime-core-tui-viewport-and-transcript.md) | TUI viewport and transcript | Accepted |
| [ADR-011](completed/ADR-011-runtime-core-tui-render-loop-and-lifecycle.md) | TUI render loop and lifecycle | Accepted |
| [ADR-012](completed/ADR-012-runtime-core-tui-deployment-gate.md) | TUI deployment gate | Accepted |
| [ADR-014](completed/ADR-014-runtime-core-policy-dedup-and-enforcement.md) | Policy dedup and enforcement | Accepted |
| [ADR-015](completed/ADR-015-local-endpoint-text-protocol-default.md) | Local endpoint text-protocol default | Accepted |
| [ADR-016](completed/ADR-016-local-tool-loop-guard-and-correction.md) | Local tool-loop guard and correction | Accepted |
| [ADR-017](completed/ADR-017-append-terminal-single-session.md) | Append-terminal single session | Superseded by ADR-018 |
| [ADR-019](completed/ADR-019-adr-018-follow-up-correctness-cutover-cleanup.md) | ADR-018 subsequent correctness cutover | Superseded by ADR-027 |
| [ADR-020](completed/ADR-020-looping-architecture-enriched-response-correctness.md) | Looping architecture enriched response | Accepted |
