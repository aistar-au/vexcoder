---
applyTo: "**"
---

This repository is a Rust and Git focused development workspace.

## Language and tone

- Use neutral engineering language throughout all generated text.
- Do not mention model vendors, assistant products, or product branding in pull
  request titles, pull request bodies, commit messages, code comments, or
  documentation unless the change is explicitly about such an integration.
- Keep wording repository-focused, task-focused, and implementation-focused.
- Prefer active voice and concrete nouns over vague abstractions.

## Change philosophy

- Understand the current behavior before editing.
- Prefer the smallest safe change that fully addresses the problem.
- Preserve existing architecture, naming, and module boundaries unless the task
  explicitly asks for restructuring.
- Do not introduce speculative refactors while fixing unrelated defects.
- Keep diffs focused. Avoid formatting-only edits and unrelated history rewrites
  unless explicitly requested.

## Pull request body structure

Use these five sections for every non-trivial pull request:

1. **Summary** — one short paragraph describing what changed.
2. **Motivation** — the concrete reason for the change: regression, bug,
   maintenance need, missing validation, correctness issue, or developer
   workflow problem. Avoid vague motivation.
3. **Approach** — implementation choices, tradeoffs, and alternatives
   considered.
4. **Validation** — exact commands run and observed results. Include both
   targeted checks and broader validation when applicable.
5. **Risks** — follow-up work, uncertainty, compatibility concerns, and any
   paths not covered by tests.

## Rust and Git expectations

- Prefer idiomatic Rust.
- Prefer explicit error handling over panic paths in reusable code.
- Update or add tests when behavior changes.
- Do not rewrite unrelated history or perform broad formatting-only edits
  unless asked.

## Validation

Before finalizing a change, try the smallest relevant validation first. If
broader validation is needed, run the full gate:

```sh
make gate-fast
```

This runs format checks, linting, architecture boundary checks, and tests
in the same sequence as CI.

## Provenance, originality, and copyright avoidance

- Prefer original wording and original implementations.
- Do not copy third-party text or code into the repository unless it is clearly
  intended, necessary, and license-compatible.
- If an implementation appears too similar to a known external source, flag the
  risk explicitly and prefer a rewrite from first principles.
- When adapting a known algorithm or pattern, implement it in the repository's
  own style and naming conventions rather than mirroring the source.
- Do not reproduce substantial portions of documentation, tutorials, or
  examples from external projects without clear need and attribution.

### Review and PR text sanitization

- Before submitting or posting any pull request body, commit message, review
  comment, or inline annotation, scan the text for vendor names, product
  branding, promotional language, and tool-specific references.
- Remove or replace any branded terms with neutral, generic equivalents.
- The `scripts/check_forbidden_names.sh` gate enforces this at CI level. Run
  it locally before pushing.
- Automated review output (from any reviewer bot) must have its threads
  resolved or the review dismissed before merge if it contains branded
  language.

## Security considerations

- Do not introduce command injection, path traversal, or unsanitized input
  handling in any user-facing or network-facing code path.
- Treat all external input (CLI arguments, environment variables, file content,
  network payloads) as untrusted until validated.
- Avoid logging secrets, tokens, or credentials. If a change touches
  authentication or authorization paths, call out the security implications in
  the pull request.
