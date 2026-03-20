# Repository-Hosted Agent Instructions

Repository-hosted background sessions in `vexcoder` are self-contained.

- Do not bootstrap, clone, sync, or depend on private skills.
- Do not fetch or inspect `../vexdraft` just to make the background session
  work.
- Read `AGENTS.md`, `CONTRIBUTING.md`, and
  `.github/instructions/repository.instructions.md` first.
- In `AGENTS.md`, ignore the `Local bootstrap only` section and every
  `../vexdraft` reference. Those lines are for local dispatcher sessions only.
- Keep one comprehensive draft branch and one comprehensive draft PR per
  feature lane.
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
- If the hosted runner lacks an undeclared local tool that this repository does
  not provision, report the environment gap instead of installing ad hoc
  tooling inside the session.
- For hosted sessions that touch docs, instructions, agent profiles, or
  workflows, run `cargo fmt --check`, `cargo test --all-targets`, and
  `bash scripts/check_forbidden_names.sh` first. Run `make gate-fast` only if
  the required local tools are already present in the runner image. Do not try
  to install missing tools during the hosted session.
