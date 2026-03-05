# PR Remote Guardrails

Remote-side rules for PR body updates and PR review posting in `aistar-au/vexcoder`.
Use this reference with `.agents/skills/vex-remote-contract/SKILL.md` when a task
requires any GitHub write for pull request text.

## Scope

This reference governs remote posting actions only:

- Creating or updating PR bodies
- Posting PR review bodies
- Posting inline review comments
- Submitting review responses that assert implementation facts

Local drafting remains in `.agents/skills/vex-local-bash/SKILL.md`.

## Required remote rules

- GitHub writes for PR bodies and reviews must use GitHub MCP.
- Do not create local PR body artifacts (`/tmp` included).
- Present the full draft to the user and wait for explicit confirmation before
  any write API call.
- Keep PR text in the assistant response until approved, then apply via MCP.
- If MCP is unavailable, stop and request explicit user override before any
  non-MCP path.

## Evidence rules before assertion text

Before asserting implementation facts in PR text, verify against remote source:

- Struct field names: confirm exact identifiers in source at the target commit.
- Subprocess routing claims: verify across all changed files; do not infer from
  memory or ADR prose.
- CI status: only assert success when the GitHub status/check APIs show
  completed success on the head SHA.

If verification is incomplete, omit the claim and mark it as pending
verification.
