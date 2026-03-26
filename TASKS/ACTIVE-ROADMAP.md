# Active Roadmap

Single canonical source for what is active. Both `onboarding.md` Section 2b
and `TASKS/TASKS-DISPATCH-MAP.md` reference this file -- they do not duplicate it.

Updated by the merge workflow after each ADR-scoped PR lands on main.
Do not edit manually except via the standard exact-diff workflow.

Last updated: 2026-03-26

---

## Active ADRs

| ADR | Status | Remaining items | Dependency note |
| :--- | :--- | :--- | :--- |
| ADR-021 | Accepted | 17 (3 P1, 13 P2, 1 P3) | Standalone audit; findings feed later ADRs |
| ADR-022 amendment | Amended | Amendment only | Tightens milestone-1 command-execution rules relative to ADR-022 |
| ADR-022 | Proposed (milestone-1 passed) | Post-milestone G/H | Roadmap; spawns ADR-023, ADR-024, ADR-027, ADR-031 |
| ADR-023 | Locked | 11 (EL-01 through EL-11) | Gates all deterministic commands; EL-12/EL-13 done |
| ADR-024 | Proposed | 16 items | Parity-gap inventory; PA/PB/PC/PD(partial)/PE/PJ/PK/PL/PM done |
| ADR-028 | Active | Ongoing alignment | Phase 1+2 landed; facade boundary governs ADR-030/031 |
| ADR-029 | Accepted | 8 decision items to re-verify | Feeds ADR-030; verification follow-up remains in Tier 6 |
| ADR-030 | Accepted | 6 coverage requirements to re-verify | 7 invariants defined; verification evidence remains in Tier 6 |
| ADR-031 | Active (Batches A-E merged) | Verification | PRs 196/225/226/227 merged |
| ADR-032 | Active | Items 4-5 | Prompt area; items 1-3, 6-8 landed; item 9 deferred to ADR-033 |
| ADR-033 | Active (Phases 1-4 landed) | Integration follow-up | PRs 186/191/192/194/199 merged |
| ADR-034 | Active (Phase A + B-E landed) | Follow-up hardening | PRs 228/229/230 merged |

## Implementation-Complete ADRs (pending move to completed/)

| ADR | Status | Notes |
| :--- | :--- | :--- |
| ADR-013 | Accepted | All phases landed |
| ADR-018 | Superseded by ADR-027 | Retained for history |
| ADR-025 | Complete | PI-09 through PI-12 all delivered |
| ADR-026 | Complete | PI-13 through PI-16 all delivered |
| ADR-027 | Accepted (landed) | Supersedes ADR-018/019 |

---

## Remaining Work: 60 Items Across 10 Tiers

Tiers sorted by unblocking impact -- what, if implemented first, unblocks
the most downstream work.

### Tier 1 -- Edit Loop Foundation (ADR-023) -- 11 items

These gate every deterministic command the agent can execute (/edit, /fix,
/explain, /run, /test, /review, /plan). Nothing else in the coding pipeline
works without these.

- EL-01: ContextAssembler stub -- gates EL-02, EL-03, EL-04, EL-05
- EL-07: ModelProfile struct + models/*.toml -- gates EL-08
- EL-02: ValidationSuite (cargo check/test/clippy) -- gates EL-04, EL-05, EL-10
- EL-03: EditLoop::run skeleton (turn lifecycle) -- gates all command wiring
- EL-06: src/prompts/ templates + docs updates
- EL-09: check_forbidden_names.sh CI coverage
- EL-04: /edit and /fix wired through edit loop
- EL-05: /explain, /run, /test wired
- EL-08: ModelProfile config integration (gated on EL-07 + ADR-022 Phase 1)
- EL-10: /review command (gated on EL-03)
- EL-11: /plan command (gated on EL-03; cross-ref ADR-024 PI-08)

### Tier 2 -- In-Flight PRs (merge to unblock downstream) -- 4 PRs

Already implemented and pushed. Merging clears the path for dependent work.

- PR 234: Debug-pass observations O-1 through O-9 (orphan state guard, DelegateError, sidecar index, strip-ansi, tracing, now_millis, agent name cap)
- PR 231: ADR-024 Phase D sandbox drivers (PD-01 done, PD-02 MacosSandboxExec, PD-03 DockerSandbox)
- PR 232: ADR-024 Phase F MCP runtime (PF-01 McpRegistry, PF-02 Capability::McpTool approval)
- PR 233: ADR-032/033 doc reconcile (system prompt, tool descriptions, documentation aligned)

### Tier 3 -- Sandbox and MCP Completion (ADR-024) -- 6 items

After PRs 231/232 merge, verify and fill remaining gaps.

- PD-02: MacosSandboxExec driver (in PR 231)
- PD-03: DockerSandbox driver (in PR 231)
- PF-01: McpRegistry STDIO + HTTP transports (in PR 232)
- PF-02: Capability::McpTool approval wiring (in PR 232)
- PI-06: /mcp list command (depends on PF-01/PF-02)
- PI-07: /mcp show <server> command (depends on PF-01/PF-02)

### Tier 4 -- Workspace Tools and MCP Extensions (ADR-024) -- 3 items

- PP-01: search_files, list_dir, glob_files tools (workspace exploration)
- PM-02: MCP HTTP [mcp_servers.headers] auth (extends Gap 5)
- PI-08: /plan and /context commands (cross-ref ADR-023 EL-11)

### Tier 5 -- Security Hardening (ADR-021 P1) -- 4 items

Unbounded buffers and unhandled errors that could cause crashes or resource
exhaustion.

- Item 18: Unbounded input buffer in editor (memory safety)
- Item 26: SSE parser buffer unbounded without delimiter (memory safety)
- Item 19: SSE parse failures not surfaced to UI (user-visible)
- Item 8: Post-cutover comment debt (code hygiene)

### Tier 6 -- Verification and Governance -- 3 items

Confirm already-landed work matches ADR specifications.

- ADR-029: Verify all 8 decision items (StreamEvent, ContentBlock, Delta, ApiUsage, MessageDelta, MessageStartData, chat-completions, TaskState) are implemented
- ADR-030: Verify 6 coverage requirements have test evidence
- ADR-032: Verify items 4 (character count indicator) and 5 (focus indicator) are implemented

### Tier 7 -- Code Quality (ADR-021 P2) -- 13 items

Duplication removal, race condition fixes, and design follow-ups.

- Item 9: Tool error dispatch block repeated
- Item 10: Scroll handling duplication
- Item 11: Approval input parsing duplicated
- Item 12: Diff row styling logic duplicated
- Item 13: required_tool_string variants overlapping
- Item 14: Auto-follow reconciliation repeated
- Item 15: MAX_INPUT_PANE_ROWS not applied in prod
- Item 20: edit_file TOCTOU race condition
- Item 22: StreamBlock::ToolCall deltas ignored
- Item 24: Startup event draining heuristics
- Item 25: Late StreamDelta dropped
- Item 28: Read-only intent heuristic false positives
- Item 32: KeyEventKind::Release filtering

### Tier 8 -- Tuning (ADR-021 P3) -- 1 item

- Item 33: IDLE_LOOP_BACKOFF tuning

### Tier 9 -- Post-Milestone (ADR-024 G/H + ADR-022) -- 7 items

Explicitly deferred until after milestone-1.

- PG-01: Release workflow -- Linux/macOS targets
- PG-02: Release workflow -- Windows (gnu) target
- PG-03: Package-manager tap formula
- PH-01: macOS app layer -- process management
- PH-02: macOS app layer -- keychain credential storage
- PH-03: macOS code signing + notarisation + .dmg
- ADR-022 Decision 11: Native packaging (post-milestone-1)

### Tier 10 -- Housekeeping -- 8 items

Move completed ADRs to completed/, update stale status fields.

- Move ADR-013 to completed/ (all work landed, status Accepted)
- Move ADR-018 to completed/ (superseded by ADR-027)
- Move ADR-025 to completed/ (PI-09 through PI-12 all done)
- Move ADR-026 to completed/ (PI-13 through PI-16 all done)
- Move ADR-027 to completed/ (fully landed)
- Verify ADR-028 remaining work and update status
- Update ADR-031 status to reflect all batches A-E merged
- Update ADR-033 status to reflect all phases 1-4 merged

---

## Dependency Graph

```
ADR-022 (Roadmap, milestone-1 passed)
  +-- ADR-023 (Edit Loop) -- 11/13 items remaining
  +-- ADR-024 (Parity Gaps) -- 16/56 items remaining
  |     +-- ADR-025 (Handoff Contract) -- COMPLETE
  |     +-- ADR-026 (Transport Binding) -- COMPLETE
  +-- ADR-027 (Command Sessions) -- COMPLETE
  +-- ADR-031 (UI Overhaul) -- Batches A-E merged
        +-- ADR-032 (Prompt/Context Guard) -- items 4-5 unclear
              +-- ADR-033 (Hybrid Retrieval) -- Phases 1-4 landed

ADR-029 (Stream Parser) --> ADR-030 (Orchestrator) --> ADR-031
ADR-028 (Facade) --> ADR-030 --> ADR-031
ADR-034 (Multi-Agent) --> ADR-028, ADR-030
```

---

## Completed ADRs (reference only)

| ADR | Completed | Notes |
| :--- | :--- | :--- |
| ADR-001 through ADR-020 | See adr/completed/ | Full history in completed/ directory |

---

## How this file is updated

After each ADR-scoped PR merges to main, the follow-up PR updates:

1. This file -- current phase / remaining items for the relevant ADR
2. Nothing else -- do not touch onboarding or dispatch map in the same edit

The PR body for a roadmap update uses the motivation template from
vex-local-bash/SKILL.md with ADR reference pointing to this file.
