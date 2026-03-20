# Agent Guide

This is a Rust TUI coding agent. Work directly on the code in the task
prompt — do not spend time on extended planning, file reading chains, or
loading external resources before starting implementation.

## Key directories

- `src/` — Rust crate source.
- `src/app/` — command routing, mode state, layout logic.
- `src/ui/draw/` — ANSI transcript renderer, regions, tests.
- `src/runtime/` — orchestration and task-state control.
- `tests/` — integration tests.
- `adr/` — architecture decision records (reference only, do not read
  unless the task prompt specifically mentions an ADR).

## Rules

- No proprietary brand names in code, comments, commits, or PR text.
- Every new dependency must be MIT or Apache 2.0 licensed.
- Prefer explicit state over stringly typed control flow.
- Reuse existing helpers. Avoid speculative refactors.
- When behavior changes, add or update focused tests.

## Before committing

```bash
cargo test --all-targets
bash scripts/check_forbidden_names.sh
```

Commit and push only after both pass.

## Pull requests

Use five sections: Summary, Motivation, Approach, Validation, Risks.

## For local dispatcher sessions

Local dispatcher workflows use private skills from `../vexdraft/.agents/skills/`.
See `CONTRIBUTING.md` for the full local workflow, session commands, and the
A–G post-session checklist.
