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

## Bootstrap (required — read before any action)

Before writing any review text, posting any comment, or producing any output:

1. Load this file in full from `.agents/skills/github-pr-review/SKILL.md`.
2. Load `.agents/skills/vex-remote-contract/SKILL.md` in full.
3. Read both files completely before writing or posting anything.

**Emojis are forbidden in all output.** This rule appears in the Style rules section
and is repeated here as a bootstrap guard. No emoji in any position — not in headings,
not in status indicators, not in table cells, not inline. Use plain text labels:
`CHANGES_REQUESTED`, `COMMENT`, `resolved`, `open`. No Unicode symbols as status markers
(`->`, `=>` outside code blocks, `:x:`, `:white_check_mark:`, `✅`, `❌` are all
forbidden).

**Confirmation required before any remote change.** When a prompt requests changes to
a remote branch, file, or PR, stop and present the full planned change to the user
before executing. Do not push, commit, create reviews, or call any write API until the
user has explicitly confirmed. A statement of intent is not confirmation.

**All changes must be exact diffs applied as patches.** Never rewrite a skill file or
repository file from memory or a cached copy. Fetch the current content from the remote
branch, produce a precise unified diff, present it for review, then apply only the
patch. No full-file rewrites.

---
## Style rules

These rules apply to every character of text posted to GitHub — review bodies,
inline comments, issue comments, and commit comments.

**No emoji.**
GitHub text in this repo is plain markdown. No emoji in any position:
not in headings, not in status indicators, not in table cells, not inline.
Use plain text labels instead: `CHANGES_REQUESTED`, `COMMENT`, `resolved`, `open`.

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

**Full repo slug in every review.**
Every review body, follow-up report, and exception record must identify the
repository as `owner/repo` (e.g. `aistar-au/vexcoder`) in the opening line.
Bare repo names and local path references are not permitted.

**Hunk patching method is `git apply` only.**
When code or docs must change, prepare an exact unified diff and apply it with
`git apply`. Do not use edit tools that rewrite full files and do not reconstruct
file content from memory.

**Exact unified diff format is required.**
Each patch must include the standard header and focused hunks:

```diff
diff --git a/<path> b/<path>
--- a/<path>
+++ b/<path>
@@ -<old_start>,<old_count> +<new_start>,<new_count> @@
-<old line>
+<new line>
```

**Apply flow (required):**

```sh
# 1) Write patch text to a file
cat > /tmp/<n>.patch <<'PATCH'
<exact unified diff>
PATCH

# 2) Validate patch shape and hunk alignment
git apply --check --recount /tmp/<n>.patch

# 3) Apply the patch
git apply --recount /tmp/<n>.patch
```

---

## Review body structure

Use this template exactly. Omit sections that have nothing to say.

```
## Review — <owner/repo> <branch> -> <base>

**Repo:** `<owner/repo>`
**Head:** `<short-sha>` — <commit message of HEAD>
**CI:** <current status — e.g., "0 checks registered", "passing", "failing on step X">

### Changes since last review

<One sentence or a short bulleted list of what changed since the prior review.
If this is a first review, omit this section entirely.>

### Findings

<Use one headed subsection per finding. See Finding subsection format below.>

### Comments

<Use one headed subsection per note. Same format as findings but labelled COMMENT.
If none, omit this section.>
```

---

## Finding subsection format

Each finding gets its own `####` heading with a plain-English label. No emoji,
no severity icon, no number prefix.

```markdown
#### <Short plain-English label>

**Repo:** `<owner/repo>`
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
`CHANGES_REQUESTED` or `COMMENT` in bold at the start.

```
**CHANGES_REQUESTED:** <finding in one sentence>. <Detail and action in two to three sentences.>
```

---

## Posting mechanics

### Repository sync preflight (required)

Before any review text, verification claim, or patch validation, establish
the correct branch context. Never blindly checkout `main` — doing so switches
away from the active feature branch and may verify the wrong SHA.

If the working tree is dirty when the skill is invoked, stop and report it.
Do not attempt checkout over uncommitted changes.

Always use the branch-aware preflight:

```sh
# Fetch and prune in one step regardless of target branch.
# Do not run git pull before this — local state may be stale.
git fetch origin --prune

# Checkout the actual verification target.
# For a PR review this is always the PR branch, not main.
# Only use main here when main is explicitly the verification target.
git checkout <target-branch>
# Use the already-fetched ref — avoids a second fetch and does not
# depend on local branch tracking config being set correctly.
git merge --ff-only origin/<target-branch>
```

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

- `CHANGES_REQUESTED - evidence only`: close with verifiable evidence only (for example CI pass URL, `git ls-tree` mode output).
- `CHANGES_REQUESTED - code change required`: requires a focused code/doc patch.
- `COMMENT`: apply small unambiguous fixes, or explicitly accept as a known gap with rationale.

Do not close a CHANGES_REQUESTED item with assertion text alone.

#### Deleted head branch guard

Before triaging review findings, verify the PR head branch exists on the remote.
If `git ls-remote --exit-code origin <head-branch>` returns non-zero, or if the
GitHub API reports the branch as deleted, pause all review actions and execute
**Step 0.5 — Branch Recovery (Deleted Branch)** from `vex-remote-contract/SKILL.md`
before continuing. Follow the priority ladder in Step 0.5: present the Priority 1
local CLI recovery script to the user first, wait for their verification output
(both SHAs + VERIFIED line), and only fall back to MCP `push_files` (Priority 2)
or `git apply` (Priority 3) if the user confirms local git access is unavailable.
Do not post any review outcomes until the head branch is restored and the recovered
HEAD SHA is verified against `origin/<head-branch>`.

```sh
# Check remote branch existence before triage
git fetch origin --prune
git ls-remote --exit-code origin <head-branch> \
  || echo "BRANCH MISSING — run Step 0.5 Priority 1 before proceeding"
```

### Resolution rules

- Evidence-only CHANGES_REQUESTED: collect artifact + URL + status; no code change required.
- Code-change findings: make the smallest possible patch; do not bundle unrelated refactors.
- All code-change patches must be produced as exact unified diffs against the current remote
  content, presented to the user for review, and applied via
  `git apply --check --recount` then `git apply --recount`. Do not reconstruct or rewrite files from memory.
- Commit message guideline:
  - `Address PR review follow-ups: <short summary>`

### Verification before push

```sh
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo test --all-targets
./scripts/check_forbidden_names.sh   # if present
```

### Ensure push landed (required)

After pushing review follow-up commits, verify the remote branch points to the
same commit as local `HEAD`.

For PR follow-ups, `<target-branch>` is the PR branch. Do not push review
follow-up commits directly to `main` as a default path.

```sh
git push origin <target-branch>
git fetch origin --prune

LOCAL_SHA="$(git rev-parse HEAD)"
REMOTE_SHA="$(git rev-parse origin/<target-branch>)"
test "$LOCAL_SHA" = "$REMOTE_SHA"
```

If `<target-branch>` is `main` by explicit user request, include a commit
hygiene exception record in the follow-up:

```markdown
### Commit Hygiene Exception
- Repo: `<owner/repo>`
- Type: direct push to `main`
- Scope: `<start-sha>..<end-sha>`
- Reason: `<why PR/merge flow was bypassed>`
- Approval: `<explicit user confirmation text or link>`
- Follow-up: `<how normal PR flow will resume>`
```

Report both SHAs in the follow-up so landing is explicit and machine-checkable.

### Follow-up report format

Use this canonical response shape:

```markdown
**Repo:** `<owner/repo>`

**Brief TODO**

1. <What was added/updated>
2. <Which review comments were addressed and how>
3. Exact diff of required changes (pushed)
```

Rules:

- Every CHANGES_REQUESTED item must include at least one artifact URL and status.
- Evidence-only outcomes must explicitly state `no code change required`.
- Code-change outcomes must include the exact unified diff.
- After push, confirm local `HEAD` equals `origin/<target-branch>` and report both SHAs.
- If `main` was updated directly, include the `Commit Hygiene Exception` block.

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
- Do not claim a CHANGES_REQUESTED item is resolved without evidence (artifact URL, status, or exact patch).
- Do not use `main` as `<target-branch>` for routine PR follow-up pushes.
- Do not skip the Bootstrap section. Load and read both skill files before any output.
- Do not make any remote change (push, commit, file write, PR creation, review post) without
  first presenting the planned change and receiving explicit user confirmation. A statement of
  intent is not confirmation.
- Do not rewrite or reconstruct any file from memory or a cached copy. Fetch the current remote
  content, produce an exact unified diff, present it, and apply the patch. Full-file rewrites
  are not permitted.
- Do not skip `git apply --check --recount` before applying a patch.
- Do not use non-diff editing methods for hunk-level changes.
- Do not triage or post review outcomes when the PR head branch is deleted or missing.
  Execute Step 0.5 branch recovery first (Priority 1 local CLI, then Priority 2 MCP
  push_files, then Priority 3 git apply as absolute last resort) and verify the
  restored HEAD before continuing.
