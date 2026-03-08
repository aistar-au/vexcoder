# Contributing

See [CONTRIBUTING.md](../../CONTRIBUTING.md) at the repository root for the full contributor guide, including the source map, verification protocol, and hard rules.

For agent and automated contributor bootstrap, see [AGENTS.md](../../AGENTS.md).

## Short checklist

Before opening a pull request:

- Run `make gate-fast` and confirm it is green (`cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo test --all-targets`).
- If you added files, update `REPO-RAW-URL-MAP.md` in the companion dispatcher repository.
- For changes to `src/` or `tests/`, run the pre-push debugger described in `AGENTS.md` before pushing.
- Merge to `main` via merge commit only. No squash, no rebase.
