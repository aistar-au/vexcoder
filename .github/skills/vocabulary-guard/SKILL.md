---
name: vocabulary-guard
description: "Enforce vocabulary and formatting constraints in agent-authored output. Use when: writing commit messages, review comments, PR text, code comments, plan text, or any prose that will be published to the repository. Bans the word dead, bans emojis and Unicode symbols, bans proprietary brand names."
---

# Vocabulary Guard

## When to Use

- Before committing any agent-authored prose (commit messages, comments, docs).
- Before posting review comments or PR text.
- When editing existing text that may contain banned terms.

## Banned Word: "dead"

The standalone word "dead" (case-insensitive word boundary match `\bdead\b`)
is banned in all agent-authored output.

### Approved Replacements

| Banned phrase | Replacement |
|---|---|
| dead code | unused code, unreferenced code, code that is never called |
| dead letter | obsolete, inoperative |
| dead end | unreachable path, terminal state |
| dead reference | stale reference, dangling reference |
| dead lock | (use "deadlock" as one word for the technical term) |

The Rust compiler lint `dead_code` may appear only when quoting compiler
output verbatim.

## Emoji and Unicode Symbol Ban

All agent-authored output must use plain ASCII text and standard Markdown.
No emojis, no Unicode status symbols, no decorative characters.

Banned categories:
- Emoji (U+1F600-U+1F64F, U+1F300-U+1F5FF, etc.)
- Dingbats (U+2700-U+27BF) when used as status indicators
- Miscellaneous symbols used as checkmarks, warnings, or status

Allowed:
- Standard Markdown formatting (bold, italic, headers, lists, code blocks)
- ASCII punctuation and standard Unicode text characters
- Technical Unicode in source code (e.g., the cosmic marker U+2726 in the
  TUI renderer) is not affected by this rule

## Forbidden Brand Names

Run `scripts/check_forbidden_names.sh` to check for banned terms. In normal
prose, rewrite references using neutral descriptions:

| Concept | Allowed phrasing |
|---|---|
| Hosted coding agent | "the hosted coding agent" |
| Profile-pinned model | "the profile-pinned model" |
| Automated reviewer | "the automated reviewer" |
| Hosted runtime | "the hosted runtime" |

## Validation

After writing any prose, verify:
1. `grep -i '\bdead\b'` returns no matches in agent-authored text.
2. No emoji or Unicode status symbols present.
3. `bash scripts/check_forbidden_names.sh` passes.
