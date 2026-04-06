---
name: pr-body-review
description: "Validate and rewrite pull request bodies. Use when: creating a PR, editing a PR body, reviewing PR text, or preparing a PR for merge. Enforces mandatory Risks section, bans the word dead, bans emojis and Unicode status symbols, and checks forbidden brand names."
---

# Pull Request Body Review

## When to Use

- Before posting or updating any PR body via the GitHub API.
- After an automated reviewer or bot edits a PR body.
- When rewriting a PR body to satisfy repository conventions.

## Mandatory Sections

Every non-trivial PR body must contain these five sections in order:

1. `## Summary`
2. `## Motivation`
3. `## Approach`
4. `## Validation`
5. `## Risks`

The `## Risks` section is mandatory. When no specific risk applies, write:
"No identified risks. The change is isolated to the named modules and CI
confirms correctness."

A PR body that omits `## Risks` is incomplete and must not be posted.

## Banned Content

### Word ban: "dead"

The word "dead" (case-insensitive, as a standalone word) is banned in all
agent-authored text including PR bodies, review comments, commit messages,
and inline findings. Use alternatives:

- "unused code" instead of "dead code"
- "unreferenced" instead of "dead reference"
- "obsolete" instead of "dead end"
- "unreachable" instead of "dead path"

The Rust compiler lint name `dead_code` may appear only when quoting
compiler output verbatim.

### Emoji and Unicode symbol ban

Do not use emojis or Unicode status symbols in PR bodies, review text,
commit messages, inline comments, plan text, or findings. Use plain ASCII
and standard Markdown only. This includes:

- No checkmark emojis, warning signs, or status indicators
- No decorative Unicode symbols
- Standard Markdown formatting (bold, headers, lists) is acceptable

### Forbidden brand names

Do not introduce proprietary product names, assistant-brand terms,
provider-name terms, model-family terms, or editor-brand terms in PR text
unless a literal path, URL, command, or quoted log line requires the exact
string. Run `scripts/check_forbidden_names.sh` to validate.

## Validation Procedure

1. Verify all five sections are present with `## ` headers.
2. Search for `\bdead\b` (case-insensitive) and replace with alternatives.
3. Search for emoji and Unicode status characters and remove them.
4. Run `bash scripts/check_forbidden_names.sh` against the PR body text.
5. Ensure neutral engineering language throughout.
