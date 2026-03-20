# Agent Guide

Read this file first, then bootstrap the required private skills before
implementation.

## Local bootstrap

- Load `../vexdraft/.agents/skills/vex-local-bash/SKILL.md`.
- Load `../vexdraft/.agents/skills/vex-remote-contract/SKILL.md`.
- Load `../vexdraft/.agents/skills/vex-rust-arch/SKILL.md` for Rust changes.
- Read `CONTRIBUTING.md`, especially the `Remote Agent Sessions` section and
  the A-G post-session workflow.

For repository-hosted background sessions, use the synchronized skill tree from
the setup workflow or the attached repository API fallback instead of assuming a
local adjacent checkout.

## Key directories

- `src/` — Rust crate source.
- `src/app/` — command routing, mode state, layout logic.
- `src/ui/draw/` — ANSI transcript renderer, regions, tests.
- `src/runtime/` — orchestration and task-state control.
- `tests/` — integration tests.
- `adr/` — architecture decision records for task-state, UI, and workflow
  contracts.

## Rules

- If the prompt touches `src/app/layout.rs`, `src/ui/render.rs`,
  `src/ui/draw/**`, `src/app/tests.rs`, `.github/agents/**`,
  `.github/instructions/**`, `CONTRIBUTING.md`, or workflow/docs files tied to
  the same feature lane, treat it as one comprehensive task. Do not split the
  same lane across multiple overlapping draft branches or PRs.
- Reuse or consolidate existing related draft PRs before starting a new one.
- No proprietary brand names in code, comments, commits, or PR text.
- Every new dependency must be MIT or Apache 2.0 licensed.
- Prefer explicit state over stringly typed control flow.
- Reuse existing helpers. Avoid speculative refactors.
- When behavior changes, add or update focused tests.

## Before committing

```bash
cargo fmt --check
cargo test --all-targets
bash scripts/check_forbidden_names.sh
```

Commit and push only after both pass.

Run `make gate-fast` when the branch touches layout, renderers, tests,
workflows, or documentation tied to the same feature lane.

## Pull requests

Use five sections: Summary, Motivation, Approach, Validation, Risks.

## For local dispatcher sessions

Local dispatcher workflows use private skills from `../vexdraft/.agents/skills/`.
See `CONTRIBUTING.md` for the full local workflow, session commands, and the
A–G post-session checklist.

- Tail remote logs with the unique session or PR identifier:
  `gh agent-task view <session-id-or-pr> --log --follow`
- Promote remote agent output onto a `dispatcher/vexcoder-...` branch before
  commit-debug, CI watch, and PR preparation.
- Keep the paragraph-renderer model pinned in the agent profile rather than
  passing a model flag at invocation time. If the hosting surface ignores the
  profile pin, record that behavior instead of silently changing the command.
