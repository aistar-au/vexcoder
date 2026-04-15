# Dependency Upgrades

This repository keeps direct third-party version requirements in the root
`Cargo.toml` `[workspace.dependencies]` table. Workspace members inherit those
entries with `workspace = true`, so most dependency upgrades stay confined to
one manifest section instead of being repeated across crate manifests.

Cargo's workspace dependency inheritance model (stabilised in Rust 1.64,
documented in the [Cargo Reference][cargo-ws-deps]) is the industry-standard
centralization path: one manifest section owns every version requirement, and
member crates inherit with `workspace = true`. Future version bumps require
touching exactly one line in `Cargo.toml`, not N lines across N crates.

[cargo-ws-deps]: https://doc.rust-lang.org/cargo/reference/workspaces.html#the-dependencies-table

## Policy

- Prefer Cargo's default semver requirements such as `"1"` or `"0.39"` over
  custom upper bounds unless a documented compatibility reason requires a
  narrower range.
- Use `cargo upgrade` to change manifest requirements. Use `cargo update` to
  refresh `Cargo.lock`. They solve different parts of an upgrade.
- Avoid hard-coding crate versions in docs unless behavior truly depends on a
  specific release. Point readers to the root `Cargo.toml` instead.
- Keep version-sensitive APIs behind local seam files rather than scattering
  crate-specific workarounds through unrelated modules.
- Run `make deps-deny` before starting any upgrade batch to establish a clean
  baseline from the RustSec advisory database and license graph.

## Tool overview

Three complementary tools cover the dependency lifecycle, each solving a
distinct problem:

| Tool | Purpose | Manifest change? |
| :--- | :--- | :--- |
| `cargo deny` | Security advisories, license compliance, graph quality | No |
| `cargo outdated` | Reports which direct requirements have newer releases | No |
| `cargo upgrade` | Edits `Cargo.toml` requirement strings to latest compatible version | Yes |
| `cargo update` | Refreshes `Cargo.lock` to newest versions allowed by existing requirements | No |

These tools are not alternatives — they are complementary layers. `cargo deny`
and `cargo outdated` are read-only audits; `cargo upgrade` and `cargo update`
are the two write operations that an upgrade lane actually needs.

## Approach comparison and alternatives considered

**Dependabot / Renovate Bot** — auto-PR services that open version-bump PRs
automatically from CI. Considered and rejected for this repository because
dependency bumps frequently require coordinated source changes at seam files
(XML parsing API, tree-sitter grammar ABIs, TUI backend API). An automated PR
containing only a manifest line change would fail CI whenever an API surface
changed, leaving a broken PR in the queue. The audit/plan/apply workflow
achieves the same stale-version discovery benefit while keeping the source edit
co-authored in the same PR.

**`cargo-machete`** — scans for dependencies declared in `Cargo.toml` but
never imported by any source file. Useful as a periodic dead-dependency sweep.
Not enforced in the main CI gate today because build-dependency and
dev-dependency false positives require per-crate triage. Can be run locally:
`cargo install cargo-machete && cargo machete`.

**`cargo-audit`** — the RustSec advisory scanner from the rust-secure-code
working group. `cargo-deny`'s `[advisories]` check subsumes `cargo-audit` for
CI purposes. `cargo-audit` remains useful for quick local advisory-only scans
that do not require pulling in the full deny config.

**Pin all versions exactly** — rejected because exact pins cause every minor
crate release to generate diff noise. Rust's semver model and the single shared
`Cargo.lock` already provide reproducible builds without exact pins. The
codex-rs project (openai/codex) follows the same convention: semver ranges in
`Cargo.toml`, reproducibility from `Cargo.lock`.

**`cargo tree -d` as a hard gate** — rejected because the transitive dependency
graph legitimately carries some parallel versions (tree-sitter grammar crates,
gix family). A hard block on any duplicate version creates noisy failures
unrelated to direct dependency hygiene. `cargo-deny`'s `bans.multiple-versions
= "warn"` surfaces duplicate versions without hard-blocking the build.

## Install the tooling once

```bash
cargo install cargo-edit --locked --no-default-features --features upgrade
cargo install cargo-outdated --locked
cargo install cargo-deny --locked
```

`cargo-edit` provides `cargo upgrade`, which edits `Cargo.toml` requirements.
`cargo-outdated` reports stale direct dependencies without forcing a manifest
change.
`cargo-deny` checks security advisories, licenses, and graph quality against
the configuration in `deny.toml`.

## Standard upgrade workflow

```bash
# 0. Security and license gate — run first to establish clean baseline.
make deps-deny

# 1. Audit direct workspace dependencies for stale versions.
make deps-audit

# 2. Preview the manifest change without writing files.
make deps-plan ARGS='-p quick-xml@0.40 -p tree-sitter@0.27'

# 3. Apply the manifest change, refresh Cargo.lock, and run cargo check.
make deps-upgrade ARGS='-p quick-xml@0.40 -p tree-sitter@0.27'

# 4. Re-run the security gate after the upgrade to catch newly exposed advisories.
make deps-deny

# 5. Run the normal verification gate before pushing.
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo nextest run -j 2
cargo test --all-targets
bash scripts/check_forbidden_names.sh
```

To raise all compatible requirements in one pass, omit the package arguments:

```bash
make deps-plan
make deps-upgrade
```

For a deliberate semver-major review lane, pass the `--incompatible allow` flag
through the `ARGS` variable:

```bash
make deps-plan ARGS='--incompatible allow'
make deps-upgrade ARGS='--incompatible allow'
```

Scoped to one category:

```bash
make deps-deny ARGS="advisories"
make deps-deny ARGS="licenses"
```

## Security advisory exceptions

Advisories are tracked in `deny.toml` under `[advisories].ignore`. Every entry
MUST include a `reason` field that records:

- The crate name and version affected
- The dependency path (which direct dep pulls it in)
- Why the advisory does not affect this codebase
- The expected resolution (upstream fix / replacement crate / version bump)

Pattern (from codex-rs `deny.toml`):

```toml
[[advisories.ignore]]
id = "RUSTSEC-YYYY-NNNN"
reason = "fxhash is unmaintained; pulled in via starlark; no fixed release yet"
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
- `cargo deny check bans` surfaces any duplicate-version warnings without the
  full advisory database fetch.

## Future: automated PR creation

When the upgrade workflow matures and all active seams are well-tested, the
`scripts/upgrade-deps.sh apply` command can be wrapped in a GitHub Actions
scheduled workflow that opens a draft PR. The design prerequisite is that CI
passes on the manifest-only change for crates whose seam files have no API
churn in the release. The current Makefile targets are already structured for
this: `make deps-deny && make deps-upgrade && make gate` is a self-contained
pipeline that could run unattended.
