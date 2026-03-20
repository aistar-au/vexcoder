---
applyTo: "**"
---

## Repository-wide guidance

### Language and tone

- Use neutral engineering language throughout generated text.
- Keep wording repository-focused, task-focused, and implementation-focused.
- Prefer active voice and concrete nouns over vague abstractions.

### Change philosophy

- Understand current behavior before editing.
- Prefer the smallest safe change that fully addresses the problem.
- Preserve existing architecture, naming, and module boundaries unless the task
  explicitly asks for restructuring.
- Keep diffs focused. Avoid formatting-only edits and unrelated history rewrites
  unless explicitly requested.

### Pull request structure

Use these five sections for every non-trivial pull request:

1. Summary
2. Motivation
3. Approach
4. Validation
5. Risks

### Validation

- Start with the smallest relevant validation for the touched files.
- If the change is broad enough to justify the full local gate, run:

```sh
make gate-fast
```

### Provenance and originality

- Prefer original wording and original implementations.
- Do not copy third-party text or code into the repository unless it is clearly
  intended, necessary, and license-compatible.
- If an implementation or document reads too close to an outside source, prefer
  a rewrite from first principles and call out the provenance risk explicitly.

### Security

- Treat CLI arguments, environment variables, file content, and network payloads
  as untrusted until validated.
- Avoid introducing command injection, path traversal, or unsanitized input
  handling in user-facing or network-facing paths.
- Do not log secrets, tokens, or credentials.
