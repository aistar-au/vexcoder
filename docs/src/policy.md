# Documentation Policy

`docs/` is reserved for mdBook content and publication inputs.
Documentation language is English only (`en`).

- Required structure:
  - `docs/book.toml`
  - `docs/src/SUMMARY.md`
  - `docs/src/*.md`
- Do not place ADR files in `docs/`.
- Do not place dispatch manifests in `docs/`.
- Pull requests must keep `mdbook build docs` green.
- Pushes to `main` publish `docs/book` to GitHub Pages through `.github/workflows/docs-build-and-deploy.yml`.
- Local preview should use `mdbook serve docs --open`.

For architecture records and dispatch:

- `TASKS/ADR-README.md`
- `TASKS/ADR-*.md`
- `TASKS/completed/ADR-*.md`
