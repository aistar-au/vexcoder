---
applyTo: "**"
---

## Repository-wide guidance

### Language and tone

- Use neutral engineering language throughout generated text.
- Keep wording repository-focused, task-focused, and implementation-focused.
- Prefer active voice and concrete nouns over vague abstractions.
- In repository-hosted background sessions, use English only in agent-authored
  output.

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
  same lane unless repository-hosted sessions are explicitly sharded with
  disjoint file ownership and one shared integration branch.

### Bootstrap

- Read `AGENTS.md` first.
- For local operator sessions, use the adjacent private skill checkout
  described in `AGENTS.md`.
- For repository-hosted background sessions, stay self-contained inside this
  repository. Do not bootstrap, fetch, or depend on private skills or adjacent
  repos before editing.
- In `AGENTS.md`, repository-hosted background sessions must ignore the
  `Local bootstrap only` section and every adjacent private-skill reference.
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

- After every `gh agent-task create`, capture the new unique session id from
  the launch output and immediately tail background-session logs with:

```sh
gh agent-task view <session-id> --log --follow
```

- Prefer the unique session id over the PR number when multiple hosted runs are
  active.
- Treat 590 seconds as the hard hosted-session ceiling. Publish any code-bearing
  work and stop before the 9-minute-50-second mark. Do not plan against the
  full 10-minute wall clock.
- Treat the launch as incomplete until the tailed log confirms the session is
  staying inside this repository, avoiding `SKILL.md`, staying in English, and
  using text-only verification.
- Inspect the hosted PR and watch its checks with:

```sh
gh pr view <pr> --json headRefName,commits,statusCheckRollup
gh pr checks <pr> --watch
```

- Keep the model pinned in the agent profile rather than adding model flags at
  invocation time. If the hosting surface ignores the profile pin, report that
  behavior explicitly instead of silently changing the command.
- Do not move on to PR inspection, review, promotion, or merge work until the
  paired launch-log tail has completed and any violation has been triaged.
- In agent-authored prose, explicitly avoid every assistant-brand term,
  provider-name term, model-family term, and editor-brand term matched by
  `scripts/check_forbidden_names.sh` unless a literal path, URL, command, or
  quoted log line requires the exact string.
- In a repository-hosted session, do not read any `SKILL.md` file.
- Use text-only verification and reporting. Do not create screenshots, screen
  captures, pseudo-screenshots, parsed terminal snapshots, image artifacts, or
  temporary visual-surrogate files.
- Do not create ad hoc temporary projects or files whose only purpose is to
  simulate, capture, or restyle the UI for visual verification.
- If `rg` is unavailable, fall back to `git grep -n`, `grep -RIn`, or direct
  file reads and continue.
- If the tailed logs show private-skill bootstrap attempts, `SKILL.md` reads,
  non-English output, screenshot or pseudo-screenshot plans, temporary visual
  artifacts, or ad hoc tool installation, stop the run, record the violation,
  correct the prompt or profile, and relaunch before treating the session as
  valid.
- If a hosted-run validation step fails only because the runner lacks a local
  tool that this repository does not provision, report the environment gap
  rather than installing ad hoc tooling in-session.
- For hosted sessions that only touch docs, instructions, agent profiles, or
  workflows, start with `cargo fmt --check`, `cargo test --all-targets`, and
  `bash scripts/check_forbidden_names.sh`. Run `make gate-fast` only when the
  full toolchain is already present in the runner image.
- Do not delegate `cargo`, `cargo clippy`, `cargo test`, `cargo check`, or `make gate-fast` to another hosted agent or subagent. If those validations are needed, leave them to the local operator or CI.
- Promote remote agent output onto a `work/<topic>` branch before
  commit-debug, CI watch, and final PR preparation.
- Create or reuse a draft PR for that `work/<topic>` branch before the
  first code-bearing push, even when the launch prompt did not explicitly ask
  for PR creation.
- After every code-bearing commit or patch set on a remote lane, push
  immediately, run `git fetch origin --prune`, and confirm
  `git rev-parse HEAD == git rev-parse origin/<branch>` before continuing.
- Before every feature-branch push, fetch `origin` and rebase onto
  `origin/main` so the review branch stays on the current merge target before
  publication.
- Once a remote lane exists, treat the remote branch head as authoritative.
  Do not continue review, commit-debug, CI watch, PR text edits, or merge work
  from unpublished local-only state.
- For one feature lane that needs parallel hosted work, create one shared
  `work/<topic>` integration branch from the latest `origin/main`, then
  launch one hosted session per disjoint write set from that same base branch.
- Each hosted shard prompt must name the shard, the owned files, the
  out-of-scope files, and the integration branch it promotes into.
- Each hosted shard must report the launch base SHA, the code-bearing commit
  SHAs, and the changed-path list before handoff.
- If a hosted run opens a non-review branch or ends with only a planning
  commit and no file diff, treat it as draft-only evidence. Do not claim the
  implementation is merged until code-bearing commits are promoted onto the
  review branch.
- If `main` moves while hosted shards are still running, do not require the
  running hosted sessions to rebase in place. Refresh the shared integration
  branch locally from the latest `origin/main`, cherry-pick completed shard
  commits onto it, and relaunch only the shards whose owned files or required
  upstream dependencies changed underneath them.
- Run `vexdraft/scripts/commit-debug.py` with the configured review slot after
  pushing the review branch. Patch findings and rerun until the review
  passes.
- After fixes land, outdate or minimize automated reviewer comments where
  possible, then reply with the fixing commit when a thread remains visible.
- Keep the final PR in draft until commit-debug is clean, automated reviews are
  sanitized, and required checks are green.
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
