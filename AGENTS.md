# Agent Guide

Read this file first.

## Hosted-session short circuit

If this is a repository-hosted background session:

- Stop here before reading any local bootstrap instruction below.
- Ignore every `../vexdraft` reference in this file. Those lines are for local
  operator sessions only.
- Do not bootstrap, clone, sync, fetch, inspect, or depend on private skills,
  adjacent repos, or setup-workflow copies of the private skill tree.
- Do not read any `SKILL.md` file in a repository-hosted background session.
- Use English only in agent-authored output, including plans, logs, pull
  requests, comments, and status updates.
- Use text-only verification and reporting. Do not create screenshots, screen
  captures, pseudo-screenshots, parsed cli snapshots, image artifacts, or
  temporary visual-surrogate files.
- Do not create ad hoc temporary projects or files whose only purpose is to
  simulate, capture, or restyle the UI for visual verification.
- Stay within this repository's tracked instructions and source tree.
- Continue with the repository-hosted agent instructions file under `.github/`,
  `.github/instructions/repository.instructions.md`, and `CONTRIBUTING.md`.

## Local bootstrap only

Ignore this section in repository-hosted background sessions.

- Only local operator sessions bootstrap private skills.
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
  `src/ui/draw/**`, `src/app/tests.rs`,
  `.github/instructions/**`, `CONTRIBUTING.md`, or workflow/docs files tied to
  the same feature lane, treat it as one comprehensive task. Do not split the
  same lane across multiple overlapping draft branches or PRs unless the lane
  is intentionally sharded for repository-hosted sessions with explicit
  disjoint file ownership and one shared integration branch.
- Reuse or consolidate existing related draft PRs before starting a new one.
- For any remote code-bearing lane, create or reuse the draft PR before the
  first code-bearing push and keep the branch pushed after every code-bearing
  commit or patch set.
- Once remote work begins, treat `origin/<branch>` as authoritative. Do not
  continue from unpublished local-only commits or diffs.
- No proprietary brand names in code, comments, commits, or PR text.
- Every new dependency must be MIT or Apache 2.0 licensed.
- Prefer explicit state over stringly typed control flow.
- Reuse existing helpers. Avoid speculative refactors.
- When behavior changes, add or update focused tests.
- **`main` is read-only for agents** — never commit, merge, or push to `main`.
  All mutable work must be on a `work/vexcoder-<slug>` feature branch in a
  sandbox worktree. The only permitted `main` operation is sync:
  `git fetch origin --prune && git merge --ff-only origin/main`.
- **`gh pr merge` requires explicit user instruction** — by default, present
  merge readiness and the recommended command without executing. When the user
  explicitly instructs the agent to merge in the current conversation, execute
  `gh pr merge --merge --delete-branch` immediately without re-asking. The
  user's instruction is the confirmation.
- Release tags are a local operator step after the reviewed merge commit lands
  on `main`. Do not open a separate PR patch just to publish `v<version>`; sync
  `main`, create the annotated tag locally with `git` or `gh`, and push the tag
  from that local checkout.

## Before committing

```bash
cargo fmt --check
cargo nextest run -j 2
cargo test --all-targets
bash scripts/check_forbidden_names.sh
```

Keep `.git/hooks/pre-push` installed so the local push path re-runs
`cargo nextest run -j 2` automatically.

The `ci` workflow runs 8 parallel jobs (lint, clippy, nextest, doctest,
test-all-targets on Ubuntu; clippy+fmt, test, package on Windows) with
cargo registry and build-artifact caching for fast subsequent runs.

Commit and push only after these checks pass.

Run `make gate-fast` when the branch touches layout, renderers, tests,
workflows, or documentation tied to the same feature lane.

## Language and review hygiene

- Keep `bash scripts/check_forbidden_names.sh` green. The repository status-term
  and naming gate applies to docs, ADRs, plans, PR text, and agent-authored
  notes. Non-neutral tone words (listed in the `tone_words` array of
  `scripts/check_forbidden_names.sh`) are banned from documentation targets.
  Use neutral alternatives (e.g. "stale" or "inactive" instead of non-neutral
  lifecycle terms).
- In operator-surface prose, prefer `CLI`, `CLI app`, or `surface` over generic
  `terminal` wording unless the exact technical term is required for a crate
  name, ANSI control, terminal-size API, or quoted log line.
- PR bodies always use `Summary`, `Motivation`, `Approach`, `Validation`, and
  `Risks`. `Risks` is mandatory even for narrow cleanup or documentation lanes.

## Pull requests

Use five sections: Summary, Motivation, Approach, Validation, Risks.

## For local operator sessions

Local operator workflows use private skills from `../vexdraft/.agents/skills/`.
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
  captures, pseudo-screenshots, parsed cli snapshots, image artifacts, or
  temporary visual-surrogate files.
- Do not create ad hoc temporary projects or files whose only purpose is to
  simulate, capture, or restyle the UI for visual verification.
- After every `gh agent-task create`, identify the new unique session id and
  immediately tail logs with:
  `gh agent-task view <session-id> --log --follow`
- Treat the launch as incomplete until the tailed log confirms the session is
  staying inside this repository, avoiding `SKILL.md`, staying in English, and
  using text-only verification.
- List hosted sessions first when the identifier is unknown:
  `gh agent-task list`
- If the tailed logs show private-skill bootstrap attempts, `SKILL.md` reads,
  non-English output, screenshot or pseudo-screenshot plans, temporary visual
  artifacts, or ad hoc tool installation, stop the run, correct the prompt or
  profile, and relaunch before treating the session as valid.
- Inspect hosted PR state and watch checks with:
  `gh pr view <pr> --json headRefName,commits,statusCheckRollup`
  `gh pr checks <pr> --watch`
- Do not move on to PR inspection, review, promotion, or merge work until the
  paired launch-log tail has completed and any violation has been triaged.
- If `rg` is unavailable, fall back to `git grep -n`, `grep -RIn`, or direct
  file reads and continue.
- Promote remote agent output onto a `work/<topic>` branch before
  commit-debug, CI watch, and PR preparation.
- Create or reuse the draft PR for that review branch before the first
  code-bearing push, then keep `HEAD` in sync with `origin/<branch>` after
  every code-bearing fix.
- Before every feature-branch push, refresh from `origin` and rebase onto the
  latest `origin/main` so the review branch stays current with moving main.
- For parallel hosted work on one feature lane, use one shared
  `work/<topic>` integration branch plus one hosted shard branch per
  disjoint write set. Keep the final merge path to `main` on the shared
  integration branch only.
- If the hosted run opens a non-review branch or ends with only a planning
  commit and no file diff, treat it as draft-only evidence. Do not present the
  change as implemented until code-bearing commits are promoted onto a
  review branch.
- If `main` moves while hosted shards are running, do not force the running
  hosted sessions to rebase. Refresh the shared integration branch from the
  latest `origin/main`, cherry-pick completed shard commits onto it, and
  relaunch only the shards whose owned files were invalidated by upstream
  changes.
- Keep the paragraph-renderer model pinned in the agent profile rather than
  passing a model flag at invocation time. If the hosting surface ignores the
  profile pin, record that behavior instead of silently changing the command.
