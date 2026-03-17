---
name: vexcoder-mdbook-docs
description: Maintain public-facing vexcoder README and mdBook pages with source-verified, non-internal language.
---

# VexCoder mdBook Docs

Use this skill when the task is to refresh `vexcoder` public docs after CLI, config,
local API, auth, or TLS changes.

## Scope

- `README.md`
- `docs/src/SUMMARY.md`
- `docs/src/**/*.md`
- `book.toml` only if navigation changes require it

## Source of truth

Read these before editing:

- `src/bin/vex.rs`
- `src/config.rs`
- `src/api.rs`
- `README.md`
- `docs/src/**/*.md`
- `AGENTS.md` only to avoid contradicting current repo rules

Use the reference contract in `references/vexcoder-docs-contract.md`.

## Editing rules

- Write for someone trying to build and run the app from source.
- Prefer short sections, concrete commands, and explicit file paths.
- Keep the voice descriptive and calm. Avoid dispatcher, task, ADR, or audit language in public docs.
- Do not add unsupported setup advice or optional complexity unless the source tree requires it.
- Preserve existing headings when a small factual refresh is enough.
- If a page is mostly wrong, rewrite it completely rather than layering more stale text on top.

## Validation

After editing, run:

```bash
bash scripts/validate-docs.sh
```

The task is not complete until `mdbook build docs` succeeds.
