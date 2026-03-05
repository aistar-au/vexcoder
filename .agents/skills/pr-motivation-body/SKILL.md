---
name: pr-motivation-body
description: >
  Write a PR motivation body as plain markdown narrative text — no tables, no
  results sections, no git commands, no Rust or code language blocks. Use this
  skill whenever producing a pull request description, PR summary, or merge
  motivation for aistar-au/vexcoder. Output is prose only: what changed, why it
  changed, and which files are affected, with every referenced ADR linked as an
  inline clickable URL embedded in natural English words.
---

# PR Motivation Body

Produce the narrative body of a pull request for `aistar-au/vexcoder`.
This skill governs content, structure, and link format.

---

## Bootstrap

Before writing any text:

1. Load this file in full.
2. Load `.agents/skills/vex-remote-contract/SKILL.md` in full so you know which
   ADR governs the batch.
3. Read both files completely before producing any output.

**Confirmation required before posting.** Present the full draft to the user and
wait for explicit approval before creating or updating the PR body on GitHub.

---

## Source verification (required before asserting implementation facts)

Before writing any sentence that names a specific struct field, describes
subprocess routing, or characterises CI status, verify the claim against the
merged commit or the current branch via the GitHub MCP.

**Struct field names:** Fetch the relevant source file at the merge commit SHA
and confirm the exact field name. Do not infer field names from ADR prose or
from memory. If the ADR names a field differently from the implemented struct,
use the name in the source file and note the divergence.

**Subprocess routing claims:** Before asserting that `std::process::Command` is
used only in a single file, check every file in the changed set for direct
`Command` usage. Subprocess routing constraints may apply per-file differently;
assert only what the source confirms.

**CI status:** Do not claim anchor tests are green or CI passed unless the
GitHub PR status API returns a completed check with a success conclusion on the
head SHA. If the API returns `state: pending` or `total_count: 0`, omit all CI
claims from the body. The words “green”, “passing”, and “failing” are
prohibited regardless (see Hard prohibitions).

---

## What this skill produces

A single block of plain markdown text suitable for the GitHub PR description
field. It covers:

- One short paragraph stating the purpose of the change and the ADR that
  authorises it, with the ADR title linked as an inline URL.
- One short paragraph describing what the implementation adds or changes,
  written in plain English without referencing compiler output, test results,
  or CI state.
- A flat list of the files that are new or substantively modified, with each
  path written as a plain inline code span and no other decoration.
- A references section containing one markdown link per ADR consulted, with
  the ADR number and short title as the link text and the GitHub blob URL as
  the target.

---

## Hard prohibitions

None of the following may appear anywhere in the output:

- Tables of any kind (no `|` columns, no alignment rows).
- Results, verification, or CI sections (`cargo test` output, clippy output,
  pass/fail summaries, SHA lines, anchor test names).
- Shell commands or shell code blocks (no fenced blocks with `sh`, `bash`,
  `zsh`, or unlabelled backtick fences containing commands).
- Rust code blocks (no fenced blocks with `rust`, `rs`, or inline Rust
  snippets).
- Emoji or Unicode symbols used as status markers.
- Numbered lists. Prose only, or a flat bulleted list for file paths.
- The words “green”, “red”, “passing”, “failing” — these belong to
  verification reports, not motivation bodies.
- Any sentence that begins with a git command, a cargo invocation, or a
  raw SHA.
- Nested markdown links of the form `[[text](url)](url)`. Every link must be a
  simple `[text](url)` pair. If link text itself needs to reference a name that
  contains brackets, rewrite the text to remove them.

---

## ADR link format

Every ADR reference must appear as a markdown inline link whose link text is
natural English words — typically the ADR number and a short title phrase — and
whose URL points to the blob on `main` in this repository.

URL pattern:

```
https://github.com/aistar-au/vexcoder/blob/main/TASKS/<filename>.md
```

Examples of correct inline link usage:

> This batch implements the first phases of [ADR-023 deterministic edit
> loop](https://github.com/aistar-au/vexcoder/blob/main/TASKS/ADR-023-deterministic-edit-loop.md),
> which replaces the previous speculative rewrite strategy.

> The changes are scoped to the boundaries described in the [ADR-022 free open
> coding agent roadmap](https://github.com/aistar-au/vexcoder/blob/main/TASKS/ADR-022-free-open-coding-agent-roadmap.md).

Links must appear inline in the sentence where the ADR is first mentioned, and
also collected in the References section at the end. Do not place raw URLs
naked in the text — they must always be the `href` of a named link.

---

## Body structure

Use this template exactly. Omit any section that has nothing to say.

```
## Motivation

**Repo:** `aistar-au/vexcoder`

<Purpose paragraph. One to three sentences. State the problem this PR solves
and name the governing ADR as an inline link. Do not mention CI, test results,
or verification output.>

<Implementation paragraph. Two to four sentences. Describe what is added or
changed in plain English: new types, new behaviours, modified modules. No
compiler language, no test function names, no cargo commands. All struct field
names and subprocess routing claims must be verified against source before
inclusion (see Source verification).>

### Files changed

- `path/to/file.rs`
- `path/to/another/file.rs`
- `Cargo.toml`

### References

- [ADR-NNN Short title](https://github.com/aistar-au/vexcoder/blob/main/TASKS/ADR-NNN-filename.md)
- [ADR-NNN Second ADR if applicable](https://github.com/aistar-au/vexcoder/blob/main/TASKS/ADR-NNN-filename.md)
```

---

## Tone and length

Write as a senior contributor explaining a change to a colleague who has not
read the ADR. Be specific about what changed and why, but stay concise. The
purpose paragraph and implementation paragraph together should not exceed ten
sentences. The files list and references section are short and flat.

Do not use the phrase “this PR” more than once. Do not use filler phrases like
“this change aims to” or “we are pleased to”. State the change directly.

---

## What not to do

- Do not produce a results section, a verification section, or a CI summary.
- Do not include shell commands, cargo invocations, or git operations.
- Do not use tables.
- Do not number the files list or the references list — use plain bullet points.
- Do not inline raw SHA values or branch names in the body prose.
- Do not put the full ADR URL naked in the prose — always wrap it as a named
  link.
- Do not use emoji or Unicode symbols.
- Do not use nested markdown links (`[[text](url)](url)` is malformed — write
  `[text](url)` only).
- Do not assert struct field names, subprocess routing, or CI status without
  first verifying them against the source at the merge commit SHA via the
  GitHub MCP (see Source verification).
- Do not post or push the body to GitHub until the user has explicitly
  confirmed the draft.
