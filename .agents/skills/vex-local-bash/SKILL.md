---
name: vex-local-bash
description: >
  Local drafting skill for PR motivation bodies and PR review text in
  aistar-au/vexcoder. Use this skill to prepare markdown drafts, findings,
  inline-comment text, and review follow-up reports from local context and
  evidence. This skill does not post to GitHub; remote posting and API-backed
  verification are handled by vex-remote-contract.
---

# Vex Local Bash

Use this skill to draft PR text locally (motivation bodies, review bodies,
inline comments, and follow-up reports). Remote writes are out of scope here.

This is the canonical local drafting/review skill for the repository and
supersedes legacy PR-review naming used in earlier sessions.

---

## Bootstrap

Before writing draft text:

1. Load this file in full.
2. Load `.agents/skills/vex-remote-contract/SKILL.md` in full.

Do not post to GitHub from this skill. Hand off final posting to
`vex-remote-contract` after explicit user confirmation.

---

## Local boundary (required)

- Draft text only; no remote writes.
- Do not call PR create/update/review APIs from this skill.
- Do not create local `/tmp` PR body artifacts.
- Produce markdown in the assistant response for user approval.
- If drafting Rust code diffs/snippets, treat them as pre-format only; final
  canonical layout must be produced by `cargo fmt` in `vex-remote-contract`.
- Hand off posting, source verification, and CI-state assertions to
  `vex-remote-contract`.

---

## Style rules

These rules apply to all drafted text:

- No emoji or Unicode status symbols.
- No numbered fix lists.
- No tables with index columns.
- No nested markdown links (`[[text](url)](url)`).
- Use full repo slug (`owner/repo`) in opening lines.
- One narrative review body per pass.
- No AI product names, third-party tool names, or copyrighted product names in
  agent-authored prose. Refer to the model and agent by generic category only:
  "the coding agent", "the language model", "the remote API", "the CI system".
  Third-party tools, libraries, and platforms must be referred to by generic
  category ("the CI platform", "the version control system", "the dependency
  manager") unless the exact name is required by a code block, command, or URL.
  This rule applies to: PR bodies, review text, inline comments, findings,
  and dispatch documents.
  Excluded from this rule: command evidence blocks, terminal output, tool
  invocations, CI logs, file paths, URLs, raw URLs, commit messages, and PR
  titles.
- Do not reproduce copyrighted text from external tools, documentation pages,
  or third-party skill files in agent-authored prose or dispatches.
- When drafting or reviewing `.github/workflows/*.yml` files, do not include
  any `uses:` step that references a third-party repository and performs
  repository write operations (e.g. creating commits, branches, or PRs).
  Replace such steps with a fail-on-drift report step.
- No tables with pass, landed, or status columns in PR bodies or review text.
  No checkbox lists (`- [ ]`, `- [x]`). Bullet points are acceptable for
  listing target files and ADR-defined changes.
- In review vocabulary, do not use "blocking". Use `CHANGES_REQUESTED` instead.

---

## PR motivation rules

For PR motivation body drafts:

- No shell or Rust code blocks.
- No CI results sections.
- No pass/fail language (`green`, `red`, `passing`, `failing`).
- No sentence that begins with git/cargo commands or raw SHA.
- Keep prose concise and factual.
- The files changed list must come from `pull_request_read(get_files)` at the
  current head SHA. Do not write a file list from session memory or prior
  context. Call the API first, then draft.

ADR references must be inline markdown links to `main` blob URLs:

`https://github.com/aistar-au/vexcoder/blob/main/TASKS/<filename>.md`

---

## PR motivation template

```markdown
## Motivation

**Repo:** `aistar-au/vexcoder`

<Purpose paragraph. One to three sentences. State what problem this PR solves
and name the governing ADR as an inline link.>

<Implementation paragraph. Two to four sentences. Describe what changed in
plain English.>

### Files changed

- `path/to/file.rs`
- `Cargo.toml`

### References

- [ADR-NNN Short title](https://github.com/aistar-au/vexcoder/blob/main/TASKS/ADR-NNN-filename.md)
```

---

## PR review template

```markdown
## Review — <owner/repo> <branch> -> <base>

**Repo:** `<owner/repo>`
**Head:** `<short-sha>` — <commit message of HEAD>
**CI:** <status provided by remote guardrails verification>

### Changes since last review

<One sentence or short bulleted list. Omit on first review.>

### Findings

<One headed subsection per finding.>

### Comments

<Optional COMMENT subsection(s).>
```

---

## Finding and inline-comment formats

```markdown
#### <Short plain-English label>

**Repo:** `<owner/repo>`
**File:** `<path/to/file>` — [view on branch](<github link>)
**Commit:** `<sha>` — <commit message>
**Why this matters:** <One sentence>

<Two to four sentences: issue, evidence, required fix>

**Action:** <Imperative one-liner>
```

Inline comment:

```markdown
**CHANGES_REQUESTED:** <finding in one sentence>. <Detail and action in two to three sentences.>
```

---

## Triage and handoff

Classify each item while drafting:

- `CHANGES_REQUESTED - evidence only`
- `CHANGES_REQUESTED - code change required`
- `COMMENT`

Before handoff to remote posting:

- Confirm all required artifact URLs are present.
- Confirm repo slug, file links, and commit IDs are present.
- Mark any unverified CI/field/routing claims as pending verification.
- Require branch-currency evidence from the remote phase: latest
  `origin/main` SHA, merge-base SHA, and changed-path list.
- Require the target files list to be generated from `pull_request_read(get_files)`
  at the current head SHA, not from session memory. Any manually constructed
  file list must be verified against the API response before handoff.
- If the branch is not based on the latest `origin/main` head commit, or
  changed paths include unrelated scope, require explicit user confirmation
  before any remote write.
- If the PR introduces any `.github/workflows/*.yml` or `.agents/skills/*/SKILL.md`
  file, confirm the map update is present in the diff before handoff. The
  `doc-ref-check` CI step enforces this and will block merge if the map
  entry is missing.

---

## What not to do

- Do not post to GitHub from this skill.
- Do not claim CI or source-verification outcomes without remote verification.
- Do not use emoji, numbered fix lists, or malformed links.
- Do not generate local `/tmp` PR body files.
- Do not enforce manual Rust line wrapping in draft guidance; leave final
  layout decisions to `rustfmt`.
- Do not hand off remote write text without branch-currency and scope evidence.

## Instruction compliance (required)

User instructions must be followed exactly and completely. Partial execution,
silent omission, or re-ordering of steps is a hard stop. No instruction may be
silently dropped, deferred, or substituted without explicit user acknowledgment.

---

## Map update verification (required before handoff)

When any file is added, removed, or renamed in the branch:

- Run `bash .agents/skills/vex-remote-contract/scripts/update_repo_raw_url_map.sh`
- Verify the header count matches `git ls-files | wc -l`
- Verify each new file appears in the map table with the correct path
- Stage the update: `git add TASKS/completed/REPO-RAW-URL-MAP.md`

Do not hand off to remote posting until the map is updated and staged.

The `doc-ref-check` CI step enforces map coverage for `.github/workflows/*.yml`
and `.agents/skills/*/SKILL.md` files only. All other file types must be verified
via `--check-index` before handoff — CI will not catch missing entries for those
paths automatically.

---

## Git add scope (required)

When staging map updates or any fix-related changes:

- Never use `git add -A` — this stages all uncommitted changes in the working
  directory, not just fix-related files, and risks committing unrelated work.
- Always specify exact paths: `git add TASKS/completed/REPO-RAW-URL-MAP.md`
- For Makefile `fix` targets: the only map-related staging line should be
  `git add TASKS/completed/REPO-RAW-URL-MAP.md`, not `git add -A`.

If a diff or Makefile target contains `git add -A`, reject it and require
correction before handoff.

---

## File count verification (required after map update)

After running `update_repo_raw_url_map.sh`, verify the header count matches the
actual tracked file count:

```sh
EXPECTED=$(git ls-files | wc -l | tr -d ' ')
HEADER=$(grep 'Total tracked files:' TASKS/completed/REPO-RAW-URL-MAP.md | grep -oE '[0-9]+')
test "$EXPECTED" = "$HEADER" \
  && echo "count match: $EXPECTED" \
  || { echo "MISMATCH: map=$HEADER git=$EXPECTED"; exit 1; }
```

Do not proceed until counts match. A mismatch means the map is stale, the script
ran against a different working tree state, or the header total was hand-edited.
