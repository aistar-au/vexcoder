---
applyTo: "**"
---

## Repository-wide guidance

### Language and tone

- Use neutral engineering language throughout generated text.
- Keep wording repository-focused, task-focused, and implementation-focused.
- Prefer active voice and concrete nouns over vague abstractions.

### Change philosophy

- Understand current behavior before editing.
- Prefer the smallest safe change that fully addresses the problem.
- Preserve existing architecture, naming, and module boundaries unless the task
  explicitly asks for restructuring.
- Keep diffs focused. Avoid formatting-only edits and unrelated history rewrites
  unless explicitly requested.
- When one task spans layout, renderer, tests, docs, instructions, and review
  cleanup for the same feature lane, keep it in one comprehensive branch and
  one comprehensive draft PR. Do not split overlapping partial drafts for the
  same lane.

### Bootstrap

- Read `AGENTS.md` first.
- For local dispatcher sessions, bootstrap the private skill tree from
  `../vexdraft/.agents/skills/`.
- For repository-hosted background sessions, stay self-contained inside this
  repository. Do not bootstrap, fetch, or depend on private skills or adjacent
  repos before editing.
- In `AGENTS.md`, repository-hosted background sessions must ignore the
  `Local bootstrap only` section and every `../vexdraft` reference.
- When present, read the repository-hosted agent instructions file under
  `.github/` as part of the background-session contract.

### Pull request structure

Use these five sections for every non-trivial pull request:

1. Summary
2. Motivation
3. Approach
4. Validation
5. Risks

### Validation

- Start with the smallest relevant validation for the touched files.
- If the change is broad enough to justify the full local gate, run:

```sh
make gate-fast
```

- For Rust/UI changes, also run:

```sh
cargo fmt --check
cargo test --all-targets
bash scripts/check_forbidden_names.sh
```

### Remote agent workflow

- List hosted sessions first when the identifier is unknown:

```sh
gh agent-task list
```

- Tail background-session logs with the unique session or PR identifier:

```sh
gh agent-task view <session-id-or-pr> --log --follow
```

- Prefer the unique session id over the PR number when multiple hosted runs are
  active.
- Inspect the hosted PR and watch its checks with:

```sh
gh pr view <pr> --json headRefName,commits,statusCheckRollup
gh pr checks <pr> --watch
```

- Keep the model pinned in the agent profile rather than adding model flags at
  invocation time. If the hosting surface ignores the profile pin, report that
  behavior explicitly instead of silently changing the command.
- In agent-authored prose, explicitly avoid every assistant-brand term,
  provider-name term, model-family term, and editor-brand term matched by
  `scripts/check_forbidden_names.sh` unless a literal path, URL, command, or
  quoted log line requires the exact string.
- In a repository-hosted session, do not read any `SKILL.md` file.
- If `rg` is unavailable, fall back to `git grep -n`, `grep -RIn`, or direct
  file reads and continue.
- If a hosted-run validation step fails only because the runner lacks a local
  tool that this repository does not provision, report the environment gap
  rather than installing ad hoc tooling in-session.
- For hosted sessions that only touch docs, instructions, agent profiles, or
  workflows, start with `cargo fmt --check`, `cargo test --all-targets`, and
  `bash scripts/check_forbidden_names.sh`. Run `make gate-fast` only when the
  full toolchain is already present in the runner image.
- Promote remote agent output onto a `dispatcher/vexcoder-...` branch before
  commit-debug, CI watch, and final PR preparation.
- If a hosted run opens a non-dispatcher branch or ends with only a planning
  commit and no file diff, treat it as draft-only evidence. Do not claim the
  implementation landed until code-bearing commits are promoted onto the
  dispatcher branch.
- Run `vexdraft/scripts/commit-debug.py` with the configured review slot after
  pushing the dispatcher branch. Patch findings and rerun until the review
  passes.
- After fixes land, outdate or minimize automated reviewer comments where
  possible, then reply with the fixing commit when a thread remains visible.
- Keep PR bodies, review comments, and commit summaries free of proprietary
  product names unless a file path, URL, quoted log line, or command requires
  the exact string.
- Watch all PR checks to completion and fix any failures before merge.
- Refresh documentation and the raw URL map when the branch changes workflow,
  instructions, UI behavior, or file ownership.

### Provenance and originality

- Prefer original wording and original implementations.
- Do not copy third-party text or code into the repository unless it is clearly
  intended, necessary, and license-compatible.
- Match proprietary reference behavior through original implementation and
  neutral descriptions, not borrowed product wording or copied interface text.
- If an implementation or document reads too close to an outside source, prefer
  a rewrite from first principles and call out the provenance risk explicitly.

### Security

- Treat CLI arguments, environment variables, file content, and network payloads
  as untrusted until validated.
- Avoid introducing command injection, path traversal, or unsanitized input
  handling in user-facing or network-facing paths.
- Do not log secrets, tokens, or credentials.
