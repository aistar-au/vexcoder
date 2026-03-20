# Repository-Hosted Agent Instructions

Repository-hosted background sessions in `vexcoder` are self-contained.

- Do not bootstrap, clone, sync, or depend on private skills.
- Do not fetch or inspect `../vexdraft` just to make the background session
  work.
- Read `AGENTS.md`, `CONTRIBUTING.md`, and
  `.github/instructions/repository.instructions.md` first.
- Keep one comprehensive draft branch and one comprehensive draft PR per
  feature lane.
- Keep wording neutral and repository-focused.
- Aim for original free-license parity through first-principles design. Do not
  reuse proprietary product wording, branded visual labels, or copyrighted UI
  material.
- Do not introduce proprietary product names in code, comments, commits,
  review replies, or PR text unless a path, URL, command, or quoted log line
  requires the exact string.
- Preserve the model pin declared inside the selected agent profile. Do not add
  invocation flags to override it from the command line.
- If the hosted runner lacks an undeclared local tool that this repository does
  not provision, report the environment gap instead of installing ad hoc
  tooling inside the session.
