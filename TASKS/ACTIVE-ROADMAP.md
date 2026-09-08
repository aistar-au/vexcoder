# Active Roadmap

Single authoritative source for what is active. Both `onboarding.md` Section 2b
and `TASKS/TASKS-WORK-MAP.md` reference this file -- they do not duplicate it.

Updated by the merge workflow after each ADR-scoped PR is merged on main.
Do not edit manually except via the standard exact-diff workflow.

Last updated: 2026-04-20 (ADR-022 amendment: normalized CLI flag surface, ChatCompat CLI cutover, 10-flag normative table)

---

## Active ADRs

| ADR | Status | Remaining items | Dependency note |
| :--- | :--- | :--- | :--- |
| ADR-021 | Accepted | 0 (all items complete) | All P1/P2/P3 items complete; see Tier 6 section |
| ADR-022 amendment | Amended | Amendment only | Tightens opening-stage command-execution rules relative to ADR-022 |
| ADR-022 | Proposed (initial validation passed) | Second-stage G/H | Roadmap; spawns ADR-023, ADR-024, ADR-027, ADR-031 |
| ADR-024 | Proposed (pre-release complete) | 1 item (PG-03 tap auto-release -- next batch planned) | PA–PM and PP done; PG-01/PG-02/PG-03 template complete; PH-01/PH-02/PH-03 complete; PL-01 (pre/post-tool hooks, Gap 26) complete |
| ADR-028 | Active | Ongoing boundary alignment | Phase 1, 2, and transport extraction committed 2026-03-25; boundary tests now cover direct, grouped, multiline, and `super::`-relative `server`/`bin` imports for all inner layers |
| ADR-029 | Accepted (amended 2026-04-01) | 0 items remaining | All 8 decision items verified in Tier 5 (PR #249); Amendment adds StreamTextNormaliser boundary for embedded tool call markup (PR #305) |
| ADR-030 | Accepted | 0 items remaining | All 6 coverage requirements verified in Tier 5 (PR #249) |
| ADR-031 | Accepted (all batches A-E merged) | 0 items remaining | Status updated in Tier 9 (PR #252) |
| ADR-032 | Accepted | 0 items remaining | Items 1-8 complete; item 4-5 verified Tier 5; item 9 transferred to ADR-033 |
| ADR-033 | Accepted (all phases 1-4 merged) | 0 items remaining | Status updated in Tier 9 (PR #252) |
| ADR-034 | Accepted (all phases A-E + watch-stream merged) | 0 items remaining | Phase E2 watch-stream added: GET /v1/session-tasks/{id}/watch SSE with immediate rollup + broadcast fan-out; PR #261 closes Phase E watch-stream |
| ADR-035 | Accepted | 0 items remaining | Gap 14 `/undo` rollback strategy is now specified and implemented with binary-safe checkpoints |
| ADR-038 | Accepted (Batches D-H merged) | 0 items remaining | Phase 1: bounded context cache + opt-in auto git; Phase 1a: search lane tightening; Phase 2: disk_policy.rs + config/cache.rs; Batch C: config/load.rs -> directory module (PR #279); Batch D: operator.rs -> directory module (PR #280); Batch E/F: context_assembler split + strict disk-policy gate (PR #281); Batch G: operator policy module + disk-policy wiring (PR #282); Batch H: task-state persist extraction + WAL evaluation (PR #283) |
| ADR-039 | Proposed (Batch A merged on main) | 3 batches (B-D) | Batch A status anchors and semantic color feedback merged in PR #292; search.exclude path-boundary normalization fix in PR #293; remaining work is broader vocabulary, active indicator, and paragraph-oriented progress stream without renaming machine statuses |
| ADR-042 | Proposed (Batch A merged) | ToolPolicy wiring, config-file support | Batch A: tool registration behind approval layer + ToolPolicy enum (PR #358); remaining: config-file `tool_policy` deserialization, system-prompt policy annotation |
| ADR-043 | Proposed | 3 adoption gates | Future structured parser lane remains optional until live runtime wiring, parity coverage, and defect-reduction gates land |
| ADR-045 | Proposed (Batch 1 merged) | Batches 2+ pending | Batch 1: sole-writer enforcement in streaming.rs, tool-call dedup in projection, messages-v1 default (PR #359); remaining: promote_thinking_blocks phase signals, model_update.rs TUI-layer violations, full RuntimeSignal coverage, checkpoints, rollback markers |
| ADR-046 | Accepted (PR #378 merged) | 0 items remaining | Peer message channel: append-only JSONL sidecar per parent task, two-layer locking, facade validation, POST/GET /v1/tasks/{id}/messages routes; PeerMessagePosted RuntimeSignal stub reserved for ADR-045 follow-up |
| ADR-048 | Proposed | Pre-implementation invariants only | Permissions-overlay mode precedence, protected-path rules, untrusted-workspace demotion, and fail-closed non-interactive behavior recorded before enforcement code lands |
| ADR-048 | Proposed | Pre-implementation invariants only | Permissions-overlay mode precedence, protected-path rules, untrusted-workspace demotion, and fail-closed non-interactive behavior recorded before enforcement code lands |
| ADR-051 | Accepted | 0 (Phase 5 in this batch) | Durable working-set record, `WorkingSetRecord` restore on `/resume`, hierarchical instruction loading, reviewable memory candidates, and `JoinIndex` agent-join. Phase 1 merged in PR #444. Phases 3/4 merged in PR #445. Phase 2 merged in PR #446. |

## Implementation-Complete ADRs (moved to completed/)

| ADR | Status | Notes |
| :--- | :--- | :--- |
| ADR-013 | Accepted — moved to completed/ | All phases complete |
| ADR-018 | Superseded — moved to completed/ | Superseded by ADR-027 |
| ADR-023 | Complete | EL-01 through EL-13 all merged |
| ADR-025 | Complete — moved to completed/ | PI-09 through PI-12 all merged |
| ADR-026 | Complete — moved to completed/ | PI-13 through PI-16 all merged |
| ADR-027 | Accepted (complete) — moved to completed/ | Supersedes ADR-018/019 |
