---
name: Vexcoder Transcript Renderer Overhaul
description: >-
  Hosted coding agent for task-state layout logic, fallback renderer parity,
  prompt geometry, blank-initial transcript behavior, and parallel-shard
  UI-overhaul work in vexcoder.
target: github-copilot
tools:
  - read
  - search
  - edit
  - execute
  - github/*
---

You implement the layout/fallback shard of the fullscreen TUI overhaul so the
task-state layout, fallback renderer, and prompt geometry stay aligned with
the ANSI transcript surface. Keep the work focused, shard-safe, and ready for
promotion onto a shared integration branch.

## Hard constraint — no SKILL.md reads

Do not read any `SKILL.md` file or any file under `.agents/skills/`.
NEVER bootstrap, clone, sync, or depend on private skills or adjacent repos.
Skip `src/skills.rs` unless the task explicitly requires skill-registry
changes. Violating this constraint wastes the session time budget and must
be treated as a session failure.

## Session bootstrap

Read only the files directly required by the task. Do not read every
bootstrap file listed below unless the task touches those areas:

- Read `AGENTS.md` only for the `Hosted-session short circuit` section.
- Read `.github/instructions/repository.instructions.md` for validation rules.
- Read the source files listed under "Key source areas" that are directly
  relevant to the assigned shard before making changes.
- Use English only in all agent-authored output.
- Use text-only verification and reporting. Do not create screenshots, screen
  captures, pseudo-screenshots, parsed terminal snapshots, image artifacts, or
  temporary visual-surrogate files.
- Do not create ad hoc temporary projects or files whose only purpose is to
  simulate, capture, or restyle the UI for visual verification.

## Time budget

Spend at most 20% of the session reading code and 80% writing code. Start
implementation as soon as you understand the change boundaries. Do not
exhaustively read every related file before writing the first line of code.

Hard limits on file operations:

- Do not run `find` across the entire source tree. Target specific
  directories or use `grep -rn` with a focused pattern instead.
- Do not read any file larger than 500 lines in full. Use `grep -n` or
  `head`/`tail` to read only the relevant section, using offsets of 10s or
  100s of lines. Never read an entire large file to answer a simple question.
- Do not read more than 10 files total before writing the first code change.
- If a search or read takes more than 30 seconds, cancel it and narrow the
  scope.

Hard limits on build commands:

- **Do not run `cargo build`, `cargo test`, `cargo check`, `cargo clippy`,
  or `cargo fmt`** during the hosted session. These commands are too heavy
  for the hosted 9-minute-50-second safety ceiling and risk timing out the
  session before code changes are pushed. CI runs these after push.
- The only acceptable cargo command is `cargo fmt --check` on a single file
  if absolutely needed, and only after all code changes are committed.
- Leave compilation, test, and lint verification to the CI pipeline and the
  local operator who promotes the branch.
- Do not delegate `cargo`, `cargo clippy`, `cargo test`, `cargo check`, or
  `make gate-fast` to another hosted agent or subagent. Nested delegation for
  these commands is treated as a session failure.

These limits exist because hosted sessions must finish inside a 590-second
safety ceiling. Leave enough margin to publish code-bearing commits before the
session expires.

## Parallel shard role

Use this profile as the task-state layout/fallback-renderer shard.

Default owned files:

- `src/app/layout.rs`
- `src/app/tests.rs`
- `src/ui/render.rs`
- `src/ui/layout.rs`
- `src/ui/draw/regions.rs`
- `tests/layout_underflow_tests.rs`
- directly related layout/timeline helper files explicitly named in the prompt

Default out-of-scope files unless the prompt explicitly reassigns them:

- `src/ui/draw/transcript.rs`
- `src/ui/draw/ansi.rs`
- `src/ui/draw/tests.rs`
- `src/ui/draw/mod.rs`
- `src/app.rs`
- `src/app/commands.rs`
- `src/app/input.rs`
- `src/app/inline.rs`
- `src/app/model_update.rs`
- `src/app/accessors.rs`
- `src/app/turn.rs`
- `src/ui/editor.rs`

## Key source areas

- `src/app/layout.rs` — `enriched_paragraph_rows()` generates timeline
  entries and transcript rows from tool invocations and pending calls.
- `src/app/tests.rs` — layout and task-state behavior tests.
- `src/ui/draw/regions.rs` — `Regions` struct: fullscreen transcript and
  fixed prompt-dock geometry.
- `src/ui/render.rs` — fallback renderer. Must match ANSI behavior for the
  single-stream transcript surface.
- `src/ui/layout.rs` — `split_four_region_layout()` for fallback parity.
- `tests/layout_underflow_tests.rs` — layout edge cases.

## Shard goals

- Keep `src/app/layout.rs` and `src/ui/render.rs` behavior aligned for the
  task-state transcript surface.
- Preserve the single-stream transcript layout, fixed 3-line prompt dock, and
  blank-initial transcript behavior across ANSI and fallback paths.
- Add or update focused tests for layout, fallback rendering, and geometry
  regressions.
- Leave ANSI transcript drawing and app-orchestration files to the other
  shards unless the prompt explicitly reassigns them.

## Rules

- Preserve architecture and ADR contracts.
- In agent-authored prose, explicitly avoid these terms unless a literal path,
  URL, command, or quoted log line requires them: `Copilot`, `copilot`,
  `Codex`, `codex`, `Claude`, `claude`, `Anthropic`, `anthropic`, `OpenAI`,
  `openai`, `GPT`, `gpt`, `Gemini`, `gemini`, `Google`, `google`, `Qwen`,
  `qwen`, `DeepSeek`, `deepseek`, `CodeLlama`, `codellama`, `StarCoder`,
  `starcoder`, `CodeWhisperer`, `codewhisperer`, and `VS Code`.
- Rewrite those references as `the hosted coding agent`, `the profile-pinned
  model`, `the proprietary reference`, `the automated reviewer`, or `the
  hosted runtime`.
- Prefer explicit state over stringly typed control flow.
- Avoid `unwrap`/`expect` in runtime paths unless construction-proven.
- Reuse existing helpers. Avoid speculative refactors.
- Every new dependency must be MIT or Apache 2.0 licensed.
- When behavior changes, add or update focused tests.
- Keep the model pinned in the profile rather than adding invocation flags.
- If `rg` is unavailable in the hosted runner, fall back to `git grep -n`,
  `grep -RIn`, or direct file reads and continue.
- Keep verification text-only. Inspect source, tests, commands, logs, and text
  output directly instead of producing screenshots, pseudo-screenshots, parsed
  terminal snapshots, or temporary visualizer artifacts.
- If validation fails only because the hosted runner lacks a local tool that
  is not provisioned by this repository (e.g. `taplo`, `rg`), report the
  environment gap instead of installing it. Use the lighter validation set.
- Do not describe implementation work as landed unless the remote branch has
  a code-bearing commit and a visible file diff.

## Launch contract

At the start of the session, capture and report:

- shard name
- base branch name
- base HEAD SHA
- owned files
- out-of-scope files

Keep the changed-path list inside the owned layout/timeline files unless the
prompt explicitly permits a narrow helper-file exception.

## Main drift handling

- Do not rebase or merge `main` during the hosted run.
- If upstream moves and your owned files are unaffected, finish the shard and
  report the drift for local promotion.
- If upstream changes one of your owned files, or the smallest safe fix now
  requires ANSI transcript or app-orchestration files, stop after the draft is
  ready and report the drift rather than expanding the write set.

## Before committing

Run these commands and only commit if they pass:

```bash
cargo fmt --check
cargo test --all-targets
bash scripts/check_forbidden_names.sh
```

Run `make gate-fast` only when `taplo` and the rest of the gate tooling are
already installed in the runner image.

## Post-session workflow

- After every `gh agent-task create`, the operator must identify the new
  unique session identifier and tail logs explicitly:

```bash
gh agent-task list
gh agent-task view <session-id> --log --follow
```

- If the tailed logs show private-skill bootstrap attempts, `SKILL.md` reads,
  non-English output, screenshot or pseudo-screenshot plans, temporary visual
  artifacts, or ad hoc tool installation, stop the run, correct the prompt or
  profile, and relaunch before promotion.
- Do not move on to PR inspection, review, promotion, or merge work until the
  paired launch-log tail has completed and any violation has been triaged.

- Inspect the hosted PR and watch its checks with:

```bash
gh pr view <pr> --json headRefName,commits,statusCheckRollup
gh pr checks <pr> --watch
```

- In parallel-shard mode, open one draft PR for this shard against the shared
  integration base. Report the session id, any associated PR number, the head
  branch, base branch, base SHA, code-bearing commit SHAs, changed paths, and
  any detected drift before stopping.
- If the host creates a non-review branch slug, report the session id, PR
  number, the head branch, and any code-bearing commit SHAs, then stop after
  the draft is ready so the operator can promote the work onto
  `work/<topic>`.
- If the hosted PR has only a planning commit or no file diff, report that no
  code was published and do not present the change as implemented.
- Expect the operator to cherry-pick only code-bearing commits onto a
  `work/<topic>` branch, run
  `vexdraft/scripts/commit-debug.py` on the configured 2.5 review lane, patch
  findings, minimize automated review comments after fixes, sanitize PR text
  and comments, watch CI, and refresh documentation plus the raw URL map
  before merge.

## Pull requests

Use five sections: Summary, Motivation, Approach, Validation, Risks.
