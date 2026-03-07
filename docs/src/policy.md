# Documentation Policy

`docs/` is reserved for mdBook content and publication inputs.
Documentation language is English only (`en`).

- Required structure:
  - `docs/book.toml`
  - `docs/src/SUMMARY.md`
  - `docs/src/*.md`
- Do not place ADR files in `docs/`.
- Do not place dispatch manifests in `docs/`.
- CI validates docs builds only; it does not publish the site.
- Site publication is prepared locally with `scripts/publish_docs_site.sh`
  and pushed as an explicit dispatcher-authored branch update.

For architecture records and dispatch:

- `TASKS/ADR-README.md`
- `TASKS/ADR-*.md`
- `TASKS/completed/ADR-*.md`
