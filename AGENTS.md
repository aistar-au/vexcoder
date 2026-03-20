# Agent Guide

Read this file first.

## Hosted-session short circuit

If this is a repository-hosted background session:

- Stop here before reading any local bootstrap instruction below.
- Ignore every `../vexdraft` reference in this file. Those lines are for local
  dispatcher sessions only.
- Do not bootstrap, clone, sync, fetch, inspect, or depend on private skills,
  adjacent repos, or setup-workflow copies of the private skill tree.
- Do not read any `SKILL.md` file in a repository-hosted background session.
- Use English only in agent-authored output, including plans, logs, pull
  requests, comments, and status updates.
- Use text-only verification and reporting. Do not create screenshots, screen
  captures, pseudo-screenshots, parsed terminal snapshots, image artifacts, or
  temporary visual-surrogate files.
- Do not create ad hoc temporary projects or files whose only purpose is to
  simulate, capture, or restyle the UI for visual verification.
- Stay within this repository's tracked instructions and source tree.
- Continue with the repository-hosted agent instructions file under `.github/`,
  `.github/instructions/repository.instructions.md`, and `CONTRIBUTING.md`.

## Local bootstrap only

Ignore this section in repository-hosted background sessions.

- Only local dispatcher sessions bootstrap private skills.
- Load `../vexdraft/.agents/skills/vex-local-bash/SKILL.md`.
- Load `../vexdraft/.agents/skills/vex-remote-contract/SKILL.md`.
- Load `../vexdraft/.agents/skills/vex-rust-arch/SKILL.md` for Rust changes.
- Read `CONTRIBUTING.md`, especially the `Remote Agent Sessions` section and
  the A-H post-session workflow.

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
  same lane across multiple overlapping draft branches or PRs unless the lane
  is intentionally sharded for repository-hosted sessions with explicit
  disjoint file ownership and one shared integration branch.
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
A–H post-session checklist.

## For repository-hosted background sessions

- Stay self-contained within this repository.
- Do not fetch or load private skills.
- Ignore the `Local bootstrap only` section above and every `../vexdraft`
  reference in this file.
- Use the repository-hosted agent instructions file under `.github/`,
  `.github/instructions/`, and the checked-in agent profiles as the
  hosted-session contract.
- Do not read any `SKILL.md` file in a repository-hosted session.
- Use English only in agent-authored output.
- Use text-only verification and reporting. Do not create screenshots, screen
  captures, pseudo-screenshots, parsed terminal snapshots, image artifacts, or
  temporary visual-surrogate files.
- Do not create ad hoc temporary projects or files whose only purpose is to
  simulate, capture, or restyle the UI for visual verification.
- After every `gh agent-task create`, identify the new unique session id and
  immediately tail logs with:
  `gh agent-task view <session-id> --log --follow`
- List hosted sessions first when the identifier is unknown:
  `gh agent-task list`
- If the tailed logs show private-skill bootstrap attempts, `SKILL.md` reads,
  non-English output, screenshot or pseudo-screenshot plans, temporary visual
  artifacts, or ad hoc tool installation, stop the run, correct the prompt or
  profile, and relaunch before treating the session as valid.
- Inspect hosted PR state and watch checks with:
  `gh pr view <pr> --json headRefName,commits,statusCheckRollup`
  `gh pr checks <pr> --watch`
- If `rg` is unavailable, fall back to `git grep -n`, `grep -RIn`, or direct
  file reads and continue.
- Promote remote agent output onto a `coder/vexcoder-...` branch before
  commit-debug, CI watch, and PR preparation.
- For parallel hosted work on one feature lane, use one shared
  `coder/vexcoder-...` integration branch plus one hosted shard branch per
  disjoint write set. Keep the final merge path to `main` on the shared
  integration branch only.
- If the hosted run opens a non-coder branch or ends with only a planning
  commit and no file diff, treat it as draft-only evidence. Do not present the
  change as implemented until code-bearing commits are promoted onto a
  coder branch.
- If `main` moves while hosted shards are running, do not force the running
  hosted sessions to rebase. Refresh the shared integration branch from the
  latest `origin/main`, cherry-pick completed shard commits onto it, and
  relaunch only the shards whose owned files were invalidated by upstream
  changes.
- Keep the paragraph-renderer model pinned in the agent profile rather than
  passing a model flag at invocation time. If the hosting surface ignores the
  profile pin, record that behavior instead of silently changing the command.
