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
- No AI assistant names, competing product names, or tool brand names in any
  output. Refer to tools by generic category only: "the coding agent",
  "the language model", "the remote API", "the CI system".
  Wire protocol identifiers used as technical configuration values
  (such as protocol scheme names) are not subject to this rule.
  This rule applies to all output channels: PR bodies, review text,
  inline comments, findings, and dispatch docs.

---

## PR motivation rules

For PR motivation body drafts:

- No shell or Rust code blocks.
- No CI results sections.
- No pass/fail language (`green`, `red`, `passing`, `failing`).
- No sentence that begins with git/cargo commands or raw SHA.
- Keep prose concise and factual.

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
- If the branch is not based on the latest `origin/main` head commit, or
  changed paths include unrelated scope, require explicit user confirmation
  before any remote write.
- If the PR introduces any `.github/workflows/*.yml` or `.agents/skills/*/SKILL.md`
  file, confirm the map update is present in the diff before handoff. The
  `doc-ref-check` CI step enforces this and will block merge if the map is stale.

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
