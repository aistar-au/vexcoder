# Dependency Upgrades

This repository keeps direct third-party version requirements in the root
`Cargo.toml` `[workspace.dependencies]` table. Workspace members inherit those
entries with `workspace = true`, so most dependency upgrades stay confined to
one manifest section instead of being repeated across crate manifests.

That layout follows Cargo's workspace dependency model and keeps future upgrades
small: update the central requirement, refresh `Cargo.lock`, and only touch
source files when one of the known seam modules needs API wiring.

## Policy

- Prefer Cargo's default semver requirements such as `"1"` or `"0.39"` over
  custom upper bounds unless a documented compatibility reason requires a
  narrower range.
- Use `cargo upgrade` to change manifest requirements. Use `cargo update` to
  refresh `Cargo.lock`. They solve different parts of an upgrade.
- Avoid hard-coding crate versions in docs unless behavior truly depends on a
  specific release. When possible, point readers to the root `Cargo.toml`.
- Keep version-sensitive APIs behind local seam files rather than scattering
  crate-specific workarounds through unrelated modules.

## Install the tooling once

```bash
cargo install cargo-edit --locked --no-default-features --features upgrade
cargo install cargo-outdated --locked
```

`cargo-edit` provides `cargo upgrade`, which edits `Cargo.toml` requirements.
`cargo-outdated` reports stale direct dependencies without forcing a manifest
change.

## Standard workflow

```bash
# 1. Audit direct workspace dependencies.
make deps-audit

# 2. Preview the manifest change without writing files.
make deps-plan ARGS='-p quick-xml@0.40 -p tree-sitter@0.27'

# 3. Apply the manifest change, refresh Cargo.lock, and run cargo check.
make deps-upgrade ARGS='-p quick-xml@0.40 -p tree-sitter@0.27'

# 4. Run the normal verification gate before pushing.
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo nextest run -j 2
cargo test --all-targets
bash scripts/check_forbidden_names.sh
```

If you want to raise all compatible requirements in one pass, omit the package
arguments:

```bash
make deps-plan
make deps-upgrade
```

For a deliberate semver-major review lane, pass the cargo-upgrade flag through:

```bash
make deps-plan ARGS='--incompatible allow'
make deps-upgrade ARGS='--incompatible allow'
```

## Upgrade seams

When a crate bump needs source changes, keep them localized to these files:

- TUI stack (`ratatui`, `crossterm`, `ansi-to-tui`, `ratatui-macros`): `src/ui/tui.rs`, `src/tui_handle.rs`
- XML tool-call parsing (`quick-xml`): `src/state/conversation/tool_call_parser.rs`
- Structural indexing (`tree-sitter` and grammar crates): `src/tools/index.rs`
- Markdown rendering (`pulldown-cmark`, `syntect`): `src/ui/render/markdown.rs`
- HTTP and MCP stack (`reqwest`, `rustls`, `rmcp`): `src/api/client/mod.rs`, `src/mcp.rs`, `src/server/`

Those seams are the intended review points for future API churn. The goal is to
keep most upgrades to a handful of manifest lines plus one localized source fix
instead of many scattered call-site edits.

## Investigating impact

- `cargo tree -i <crate>` shows why one crate is in the graph and which direct
  dependency is pulling it in.
- `cargo tree -d` is useful for investigation, but it is not a hard gate in
  this repository because some ecosystems legitimately pull parallel transitive
  versions.
- `cargo metadata --no-deps --format-version 1` is the fastest way to inspect
  which workspace package owns a direct dependency.