---
name: pull-request
description: >
  Write PR motivation bodies and post PR reviews for aistar-au/vexcoder. Use
  this skill whenever producing a pull request description, posting a code
  review, writing inline review comments, triaging review findings, or updating
  a PR body. Output is plain English markdown only: no tables with index columns,
  no emoji, no numbered fix lists, no CI result sections, no shell or Rust code
  blocks. Every ADR reference is an inline clickable link. Source verification
  against the merge commit is required before asserting struct field names,
  subprocess routing, or CI status.
---

# Pull Request

Covers two surfaces: writing the PR motivation body, and writing or responding
to PR reviews. Both share the same style contract.

---

## Bootstrap

Before writing any text or posting anything:

1. Load this file in full.
2. Load `.agents/skills/vex-remote-contract/SKILL.md` in full.
3. Read both files completely before producing any output.

**Confirmation required before any remote change.** Present the full draft to
the user and wait for explicit approval before creating, updating, or posting
anything to GitHub. A statement of intent is not confirmation.

## Execution boundary (required)

- GitHub writes for PR body updates must use GitHub MCP only.
- Do not create, edit, or delete local files for PR body work (`/tmp` included).
- Do not generate local PR body artifacts.
- Keep the draft in the assistant response, then apply via MCP after approval.
- If MCP is unavailable, stop and request explicit user override before any non-MCP path.

---

## Style rules (both surfaces)

These rules apply to every character of text posted to GitHub.

**No emoji.** Use plain text labels: `CHANGES_REQUESTED`, `COMMENT`,
`resolved`, `open`. No Unicode symbols as status markers (`✅`, `❌`, `:x:`,
`:white_check_mark:`, `->`, `=>` outside code blocks).

**No numbered fix lists.** Numbered lists for open issues go stale the moment
a fix is added or resolved. Use a flat headed section per finding instead.

**No tables with index columns.** Tables must not have a `#` or sequence
column.

**English text only.** Write the word; do not substitute symbols for words.

**Full repo slug in every post.** Every review body, PR body, follow-up
report, and exception record must identify the repository as `owner/repo`
(e.g. `aistar-au/vexcoder`) in the opening line.

**No nested markdown links.** Every link must be a simple `[text](url)` pair.
`[[text](url)](url)` is malformed and prohibited.

**Single review body per pass.** Post one review comment per review pass.
Inline file comments are separate and fine, but the narrative review body must
be one block.

**No noise.** Do not restate what was already confirmed correct in a previous
review pass unless something changed.

---

## Hard prohibitions (PR body)

None of the following may appear in a PR motivation body:

- Tables of any kind.
- Results, verification, or CI sections — no `cargo test` output, clippy
  output, pass/fail summaries, SHA lines, or anchor test names.
- Shell or Rust code blocks.
- The words “green”, “red”, “passing”, “failing”.
- Any sentence that begins with a git command, a cargo invocation, or a raw SHA.
- Numbered lists — use flat bullet points only.

---

## Source verification (required before asserting implementation facts)

Before writing any sentence that names a struct field, describes subprocess
routing, or characterises CI status, verify the claim against the merged commit
or current branch via the GitHub MCP.

**Struct field names:** Fetch the relevant source file at the merge commit SHA
and confirm the exact field name. Do not infer field names from ADR prose or
from memory.

**Subprocess routing claims:** Before asserting that a mechanism is used only
in a single file, check every file in the changed set. Routing constraints may
apply per-file differently; assert only what the source confirms.

**CI status:** Do not claim tests passed or CI was clean unless the GitHub PR
status API returns a completed check with a success conclusion on the head SHA.
If the API returns `state: pending` or `total_count: 0`, omit all CI claims.
The words “green”, “passing”, and “failing” are prohibited regardless.

---

## ADR link format

Every ADR reference must be a markdown inline link whose text is natural
English words and whose URL points to the blob on `main`.

URL pattern:
```
https://github.com/aistar-au/vexcoder/blob/main/TASKS/<filename>.md
```

Links must appear inline where the ADR is first mentioned, and again in the
References section. Never place a raw URL naked in prose.

---

## PR motivation body structure

Use this template. Omit any section that has nothing to say.

```
## Motivation

**Repo:** `aistar-au/vexcoder`

<Purpose paragraph. One to three sentences. State what problem this PR solves
and name the governing ADR as an inline link. No CI claims, no test results.>

<Implementation paragraph. Two to four sentences. Describe what is added or
changed in plain English: new types, new behaviours, modified modules. All
struct field names and subprocess routing claims must be source-verified (see
Source verification above).>

### Files changed

- `path/to/file.rs`
- `Cargo.toml`

### References

- [ADR-NNN Short title](https://github.com/aistar-au/vexcoder/blob/main/TASKS/ADR-NNN-filename.md)
```

Tone: write as a senior contributor explaining the change to a colleague who
has not read the ADR. Be specific but concise. The two prose paragraphs
together must not exceed ten sentences. Do not use “this PR” more than once.
Do not use filler phrases like “this change aims to”.

---

## PR review body structure

Use this template. Omit sections that have nothing to say.

```
## Review — <owner/repo> <branch> -> <base>

**Repo:** `<owner/repo>`
**Head:** `<short-sha>` — <commit message of HEAD>
**CI:** <current status from GitHub API — e.g. “0 checks registered”>

### Changes since last review

<One sentence or short bulleted list of what changed. Omit on first review.>

### Findings

<One headed subsection per finding — see Finding format below.>

### Comments

<One headed subsection per note, labelled COMMENT. Omit if none.>
```

---

## Finding format

Each finding gets its own `####` heading with a plain-English label. No emoji,
no severity icon, no number prefix.

```
#### <Short plain-English label>

**Repo:** `<owner/repo>`
**File:** `<path/to/file>` — [view on branch](<github link>)
**Commit:** `<sha>` — <commit message>
**Why this matters:** <One sentence on the consequence if not fixed.>

<Two to four sentences: what is wrong, what the diff shows, what the fix is.
Reference specific line numbers where useful.>

**Action:** <Imperative one-liner — what the author must do.>
```

---

## Inline comment format

```
**CHANGES_REQUESTED:** <finding in one sentence>. <Detail and action in two to
three sentences.>
```

---

## Review triage classification

When responding to an existing review, classify each item before acting:

- `CHANGES_REQUESTED - evidence only`: close with a verifiable artifact URL or
  command output. No code change required.
- `CHANGES_REQUESTED - code change required`: make the smallest possible patch.
  Do not bundle unrelated changes.
- `COMMENT`: apply small unambiguous fixes, or accept as a known gap with
  rationale.

Do not close a `CHANGES_REQUESTED` item with assertion text alone.

---

## Follow-up report format

```
**Repo:** `<owner/repo>`

<Brief summary of what was addressed and how.>

- Every CHANGES_REQUESTED item includes at least one artifact URL and status.
- Evidence-only outcomes state explicitly: no code change required.
- Code-change outcomes include the exact unified diff.
- After push: confirm local HEAD equals origin/<branch> and report both SHAs.
```

---

## What not to do

- Do not post without explicit user confirmation.
- Do not use tables with index columns or numbered fix lists.
- Do not use emoji or Unicode status symbols.
- Do not split one review pass into multiple API calls.
- Do not re-confirm prior-pass findings that are resolved — note them in
  “Changes since last review” and move on.
- Do not claim a CHANGES_REQUESTED item is resolved without evidence.
- Do not use nested markdown links.
- Do not assert struct field names, subprocess routing, or CI status without
  source verification via the GitHub MCP.
- Do not include shell commands, cargo invocations, or raw SHAs in a PR body.
- Do not use the words “green”, “red”, “passing”, or “failing” in a PR body.
- Do not write PR body content to local files or `/tmp`; keep drafts in the
  assistant response and apply via GitHub MCP only.
