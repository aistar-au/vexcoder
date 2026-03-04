---
name: github-pr-review
description: >
  Write and post GitHub pull request reviews and inline comments, and respond to review
  findings with evidence-backed updates. Use this skill whenever writing a PR review
  comment, posting a code review, updating an existing review, triaging PR findings, or
  producing any GitHub-facing review text. Enforces repo style: plain English markdown,
  no emoji, no numbered fix lists, single consolidated review body.
---

# GitHub PR Review

Produce pull request reviews that are precise, scannable, and permanently accurate.
This skill governs tone, structure, and posting mechanics.

---

## Style rules

These rules apply to every character of text posted to GitHub — review bodies,
inline comments, issue comments, and commit comments.

**No emoji.**
GitHub text in this repo is plain markdown. No emoji in any position:
not in headings, not in status indicators, not in table cells, not inline.
Use plain text labels instead: `Blocker`, `Non-blocking`, `Resolved`, `Open`.

**No numbered fix lists.**
Numbered lists for open issues go stale the moment a fix is added or resolved.
Use a markdown table without a sequence column, or a flat headed section per
finding. The table must not have a `#` or index column.

**English text only.**
No symbols used as substitutes for words (`->`, `=>` outside code blocks,
`:x:`, `:white_check_mark:`, `✅`, `❌`). Write the word.

**Single review body.**
Post one review comment per review pass, not multiple separate review API calls
for the same pass. Inline file comments are separate and fine, but the narrative
review body must be one block.

**No noise.**
Do not restate what was already confirmed correct in a previous review pass
unless something changed. Prior-pass confirmations belong in a brief
"Changes since last review" line, not re-listed in full.

---

## Review body structure

Use this template exactly. Omit sections that have nothing to say.

```
## Review — <branch> -> <base>

**Head:** `<short-sha>` — <commit message of HEAD>
**CI:** <current status — e.g., "0 checks registered", "passing", "failing on step X">

### Changes since last review

<One sentence or a short bulleted list of what changed since the prior review.
If this is a first review, omit this section entirely.>

### Findings

<Use one headed subsection per finding. See Finding subsection format below.>

### Non-blocking notes

<Use one headed subsection per note. Same format as findings but labelled Non-blocking.
If none, omit this section.>
```

---

## Finding subsection format

Each finding gets its own `####` heading with a plain-English label. No emoji,
no severity icon, no number prefix.

```markdown
#### <Short plain-English label>

**File:** `<path/to/file>` — [view on branch](<raw github link>)
**Commit:** `<sha>` — <commit message>
**Why this matters:** <One sentence on the consequence if not fixed.>

<Two to four sentences of detail: what exactly is wrong, what the diff shows,
what the fix is. Reference specific line numbers or diff hunks where useful.
If a code snippet aids precision, include a fenced block.>

**Action:** <Imperative one-liner — what the author must do.>
```

Do not include a "Required action" label separate from the finding body.
Fold the action into the finding. One block per finding, no sub-bullets.

---

## Inline comment format

Inline comments on specific diff lines must follow the same style rules:
no emoji, plain text, one focused point per comment. Label the comment
`Blocker` or `Non-blocking` in bold at the start.

```
**Blocker:** <finding in one sentence>. <Detail and action in two to three sentences.>
```

---

## Posting mechanics

### Repository sync preflight (required)

Before any review text, verification claim, or patch validation, establish
the correct branch context. Never blindly checkout `main` — doing so switches
away from the active feature branch and may verify the wrong SHA.

Always use the branch-aware preflight:

```sh
# Fetch and prune in one step regardless of target branch.
# Do not run git pull before this — local state may be stale.
git fetch origin --prune

# Checkout the actual verification target.
# For a PR review this is always the PR branch, not main.
# Only use main here when main is explicitly the verification target.
git checkout <target-branch>
git pull --ff-only
```

If the working tree is dirty when the skill is invoked, stop and report it.
Do not attempt checkout over uncommitted changes.

Report the head SHA used for verification. Do not verify against stale local
content.

When updating a prior review rather than opening a fresh one:

- Post a new review body (GitHub does not support editing review bodies via API).
- Reference the prior review SHA in the opening line so the history is traceable.
- Do not re-post inline comments that are already on the PR and unresolved;
  only add new inline comments for new findings.

---

## PR Response Workflow

Use this section when addressing review comments on an existing PR.

### Triage

Fetch all reviews first and classify each item:

- `Blocker - evidence only`: close with verifiable evidence only (for example CI pass URL, `git ls-tree` mode output).
- `Blocker - code change required`: requires a focused code/doc patch.
- `Non-blocking`: apply small unambiguous fixes, or explicitly accept as a known gap with rationale.

Do not close a blocker with assertion text alone.

### Resolution rules

- Evidence-only blockers: collect artifact + URL + status; no code change required.
- Code-change findings: make the smallest possible patch; do not bundle unrelated refactors.
- Commit message guideline:
  - `Address PR review follow-ups: <short summary>`

### Verification before push

```sh
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo test --all-targets
./scripts/check_forbidden_names.sh   # if present
```

### Follow-up report format

Use this canonical response shape:

```markdown
**Brief TODO**

1. <What was added/updated>
2. <Which review comments were addressed and how>
3. Exact diff of required changes (pushed)
```

Rules:

- Every blocker must include at least one artifact URL and status.
- Evidence-only outcomes must explicitly state `no code change required`.
- Code-change outcomes must include the exact unified diff.

---

## What not to do

- Do not post a summary table with a `#` or index column.
  Tables that list findings by number go stale.
- Do not use `🔴`, `🟡`, `✅`, `❌` or any Unicode symbol as a status marker.
- Do not split one review pass into multiple `github:create_pull_request_review` calls.
- Do not re-confirm findings from the prior review that are resolved.
  Mark them resolved in "Changes since last review" and move on.
- Do not use the phrase "round N" as a section heading — use the head SHA instead.
- Do not add a numbered summary table. Use a plain findings section.
- Do not claim a blocker is resolved without evidence (artifact URL, status, or exact patch).
