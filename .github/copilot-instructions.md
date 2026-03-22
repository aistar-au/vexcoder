# Repository-Hosted Agent Instructions

Repository-hosted background sessions in `vexcoder` are self-contained.

- Do not bootstrap, clone, sync, or depend on private skills.
- Do not fetch or inspect `../vexdraft` just to make the background session
  work.
- Read `AGENTS.md`, `CONTRIBUTING.md`, and
  `.github/instructions/repository.instructions.md` first.
- In `AGENTS.md`, ignore the `Local bootstrap only` section and every
  `../vexdraft` reference. Those lines are for local operator sessions only.
- Do not read any `SKILL.md` file in a repository-hosted session.
- Use English only in all agent-authored output.
- Use text-only verification and reporting. Do not create screenshots, screen
  captures, pseudo-screenshots, parsed terminal snapshots, image artifacts, or
  temporary visual-surrogate files.
- Do not create ad hoc temporary projects or files whose only purpose is to
  simulate, capture, or restyle the UI for visual verification.
- Keep one comprehensive draft branch and one comprehensive draft PR per
  feature lane.
- For any remote code change, create or reuse that draft PR before the first
  code-bearing push and keep the branch pushed after every code-bearing commit
  or patch set.
- Treat `origin/<branch>` as authoritative once remote work begins. Do not
  leave unpublished local-only commits or diffs on a remote feature lane.
- If local `HEAD` diverges from `origin/<branch>`, stop, push or resync, then
  continue from the verified remote SHA only.
- Keep wording neutral and repository-focused.
- Aim for original free-license parity through first-principles design. Do not
  reuse proprietary product wording, branded visual labels, or copyrighted UI
  material.
- Do not introduce proprietary product names in code, comments, commits,
  review replies, PR text, plan text, or status updates unless a path, URL,
  command, or quoted log line requires the exact string.
- Treat every assistant-brand term, provider-name term, model-family term, and
  editor-brand term matched by `scripts/check_forbidden_names.sh` as
  explicitly banned in agent-authored prose unless one of the exact-string
  exceptions above applies.
- If you need to refer to one of those concepts in normal prose, rewrite it as
  `the hosted coding agent`, `the profile-pinned model`, `the proprietary
  reference`, `the automated reviewer`, or `the hosted runtime`.
- Preserve the model pin declared inside the selected agent profile. Do not add
  invocation flags to override it from the command line.
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
- Prefer the unique session id over the PR number when tailing logs during
  concurrent hosted runs.
- Do not move on to PR inspection, review, promotion, or merge work until the
  paired launch-log tail has completed and any violation has been triaged.
- If `rg` is unavailable, fall back to `git grep -n`, `grep -RIn`, or direct
  file reads and continue.
- If the hosted runner lacks an undeclared local tool that this repository does
  not provision, report the environment gap instead of installing ad hoc
  tooling inside the session.
- For hosted sessions that touch docs, instructions, agent profiles, or
  workflows, run `cargo fmt --check`, `cargo test --all-targets`, and
  `bash scripts/check_forbidden_names.sh` first. Run `make gate-fast` only if
  the required local tools are already present in the runner image. Do not try
  to install missing tools during the hosted session.
- If the host opens a non-review branch, stop after the draft is ready and
  report the session id, any associated PR number, the head branch, and any
  code-bearing commit SHAs so the operator can promote the diff.
- If the hosted run finishes with only a planning commit or no file diff,
  report that no code was published and do not describe the implementation as
  landed.
- Do not delegate `cargo`, `cargo clippy`, `cargo test`, `cargo check`, or
  `make gate-fast` to another hosted agent or subagent. Leave those commands to
  the local operator or CI because nested delegation is unreliable in the
  hosted runtime.
