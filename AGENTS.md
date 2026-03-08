# Agent Bootstrap — vexcoder

> Read this file first. It is the authoritative session bootstrap for all dispatchers
> (Claude, Copilot, Codex, and equivalent agents) working in this repository.

---

## Skill location

Skills, task manifests, and dispatcher docs live in the **sibling private repo** at
`../vexdraft`. The `vex-branch-contract` assumes this path; it must exist as a sibling
checkout.

| Path | Contents |
| :--- | :--- |
| `../vexdraft/agents/vexcoder/onboarding.md` | Full dispatcher onboarding — read before any task |
| `../vexdraft/agents/vexcoder/skills/vex-rust-arch/` | Rust architecture and ADR compliance skill |
| `../vexdraft/agents/vexcoder/skills/vex-remote-contract/` | Remote dispatch contract and batch gate skill |
| `../vexdraft/agents/vexcoder/skills/vex-local-bash/` | Local bash, `make gate-fast`, and taplo-safe skill |
| `../vexdraft/tasks/vexcoder/` | Task manifests and dispatch map |

---

## Active dispatch — ADR-021 through ADR-024

These four ADRs are currently open and driving all dispatch work.

| ADR | Status | Batch state |
| :--- | :--- | :--- |
| [ADR-021](docs/adr/ADR-021-codebase-audit-dead-weight-duplication-shared-code-opportunities.md) | Accepted — follow-up maintenance items remain | ongoing |
| [ADR-022 amendment](docs/adr/ADR-022-amendment-2026-03-03.md) | Proposed | — |
| [ADR-022](docs/adr/ADR-022-free-open-coding-agent-roadmap.md) | Proposed | — |
| [ADR-023](docs/adr/ADR-023-deterministic-edit-loop.md) | Locked | **EL-04 is the active next batch** |
| [ADR-024](docs/adr/ADR-024-zero-licensing-cost-agent-parity-gaps.md) | Proposed | — |

### ADR-023 — current batch: EL-04

EL-03 merged via PR #43 (`dispatcher/adr-023-el-03-skeleton`). EL-04 is unblocked.

EL-04 scope:
- `TuiMode` changes required by the edit loop
- Reentrancy anchor test deferred from EL-03

Branch naming convention: `dispatcher/adr-023-el-04-<slug>`.

### ADR-021 — corrected quota parameters

These values supersede any earlier figures in the codebase or ADR text:

| Parameter | Correct value |
| :--- | :--- |
| `max_tokens` | 3200 |
| `READ_FILE_SNAPSHOT_MAX_CHARS` | 4000 |
| remote `max_tool_rounds` | 18 |

`saturating_add` across tool rounds causes overcounting of shared context; the correct
semantics for input token accumulation is overwrite (only the final API call's input
token count).

---

## Verification protocol

Every task requires exactly one failing anchor test before dispatch begins.
No anchor, no dispatch.

```bash
# Anchor must fail before work starts, pass after
cargo test <anchor_name> -- --nocapture

# No regressions
cargo test --all-targets

# Architecture gates
bash scripts/check_no_alternate_routing.sh
bash scripts/check_forbidden_imports.sh

# Local gate (macOS: routes taplo through scripts/taplo_safe.sh)
make gate-fast
```

---

## Hard rules for dispatchers

- **Read-only declaration first.** Begin every session with an explicit read-only
  declaration before any tool call that writes to the repository.
- **No direct pushes to `main`.** All batch promotions use `--no-ff` merge commits per
  `vex-branch-contract`.
- **`/clear` requires cancellation.** The `/clear` command must cancel any
  `active_edit_loop` before resetting context (ADR-024 Gap 14).
- **`Arc::clone` placement.** `Arc::clone` must be declared before `tokio::spawn`
  closures reference accumulated values.
- **Hard Rule 22 push override threshold.** The local CLI push override is payload-size
  based (≥ 50 KB), not formatter-availability based.
- **Input token accumulation.** Use overwrite semantics for input token counts across
  tool rounds, not `saturating_add`.

---

## Full ADR index

[docs/adr/ADR-README.md](docs/adr/ADR-README.md)

## Full contributor guide

[CONTRIBUTING.md](CONTRIBUTING.md)
