---
description: "Enforce PR body structure and banned vocabulary in pull request descriptions, commit messages, and agent-authored prose. Use when: creating PRs, writing PR bodies, reviewing PR text."
applyTo: "**/*.md"
---

# PR Hygiene

## PR Body Structure

Every pull request body must contain these five sections in order:

1. **Summary** — One-paragraph overview of the change.
2. **Motivation** — Why this change is needed.
3. **Approach** — How the change is implemented.
4. **Validation** — How the change was verified (tests, manual checks).
5. **Risks** — What could go wrong. **Mandatory even for trivial changes.**

If `Risks` is missing, the PR is incomplete. Add at least one concrete risk
or state "No significant risks identified" with justification.

## Banned Vocabulary

The following words are banned from all documentation, ADRs, PR bodies,
commit messages, TASKS, and agent-authored prose:

- **Lifecycle terms:** Use "stale", "inactive", "expired", or "terminated"
  instead of non-neutral lifecycle words flagged by
  `scripts/check_forbidden_names.sh` (Pass 4: tone_words array).
- **Brand names:** No proprietary assistant, model, or editor brand names
  unless quoting a path, URL, or log line verbatim.

Run `bash scripts/check_forbidden_names.sh` before every commit to verify
compliance.
