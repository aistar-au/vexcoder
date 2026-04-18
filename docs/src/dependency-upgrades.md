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

That central manifest is necessary, but it does not create a runtime "crate
version variable" inside Rust source. Versions are resolved at compile time.
When an upgrade needs source edits, start from the root `Cargo.toml`
`[workspace.metadata.upgrade-seams]` and `[workspace.metadata.upgrade-notes]`
tables and keep the fallout inside those local seam files.

[cargo-ws-deps]: https://doc.rust-lang.org/cargo/reference/workspaces.html#the-dependencies-table

## Policy

- Prefer Cargo's default semver requirements such as `"1"` or `"0.39"` over
  custom upper bounds unless a documented compatibility reason requires
  essential version constraints.
- Use `cargo upgrade` to change manifest requirements. Use `cargo update` to
  refresh `Cargo.lock`. They solve different parts of an upgrade.
- Avoid hard-coding crate versions in docs unless behavior truly depends on a
  specific release. Point readers to the root `Cargo.toml` instead.
- Start every source-impacting upgrade by checking
  `[workspace.metadata.upgrade-seams]` and `[workspace.metadata.upgrade-notes]`
  in the root `Cargo.toml`. Those tables are the repository-maintained map of
  which files are supposed to absorb API churn.
- Keep version-sensitive APIs behind local seam files rather than scattering
  crate-specific workarounds through unrelated modules.
- Run `make deps-deny` before starting any upgrade pass to establish a clean
  baseline from the RustSec advisory database and license graph.

## Tool overview

Three complementary tools cover the dependency lifecycle, each solving a
distinct problem:

| Tool | Purpose | Manifest change? |
| :--- | :--- | :--- |
| `cargo deny` | Security advisories, license compliance, graph quality | No |
| `cargo machete` | Detects unused direct dependencies in `Cargo.toml` | No |
| `cargo outdated` | Reports which direct requirements have newer releases | No |
| `cargo semver-checks` | Verifies library crate API compatibility after upgrades | No |
| `cargo upgrade` | Edits `Cargo.toml` requirement strings to latest compatible version | Yes |
| `cargo update` | Refreshes `Cargo.lock` to newest versions allowed by existing requirements | No |

These tools are not alternatives — they are complementary layers. `cargo deny`
and `cargo outdated` are read-only audits; `cargo upgrade` and `cargo update`
are the two write operations that an upgrade pass actually needs.

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
unused by source files. Integrated in `make deps-audit`: running
`make deps-audit` now reports both unused dependencies (machete) and stale
version requirements (outdated) in one pass. Not enforced in the main CI gate
because build-dependency and dev-dependency false positives require per-crate
triage.

**`cargo-semver-checks`** — verifies that changes to library crate APIs do not
introduce semver-incompatible breaking changes. Integrated in `make deps-upgrade`:
after applying a manifest upgrade, `cargo-semver-checks` compares the library
crate APIs against `origin/main` and reports any regressions. Advisory only:
exits non-zero on breakage but does not block the upgrade step.

**`cargo-audit`** — the RustSec advisory scanner from the rust-secure-code
working group. `cargo-deny`'s `[advisories]` check subsumes `cargo-audit` for
CI purposes. `cargo-audit` remains useful for quick local advisory-only scans
that do not require pulling in the full deny config.

**Pin all versions exactly** — rejected because exact pins cause every minor
crate release to generate diff noise. Rust's semver model and the single shared
`Cargo.lock` already provide reproducible builds without exact pins. A public
Rust coding-assistant repository uses the same convention: semver ranges in
`Cargo.toml`, reproducibility from `Cargo.lock`.

That same reference repository is also a useful example of what not to
over-abstract: it centralizes manifest versions, but still keeps `serde`
derives and many Tokio attribute macro call sites direct in source. That
matches the approach here: add seams where they materially reduce churn, and
leave compile-time annotations where they are already explicit and local.

**`cargo tree -d` as a hard gate** — rejected because the transitive dependency
graph legitimately carries some parallel versions (tree-sitter grammar crates,
gix family). A hard block on any duplicate version creates noisy failures
unrelated to direct dependency hygiene. `cargo-deny`'s `bans.multiple-versions
= "allow"` keeps those transitive splits from blocking the build.

## Install the tooling once

```bash
cargo install cargo-edit --locked --no-default-features --features upgrade
cargo install cargo-outdated --locked
cargo install cargo-deny --locked
cargo install cargo-machete --locked
cargo install cargo-semver-checks --locked
```

`cargo-edit` provides `cargo upgrade`, which edits `Cargo.toml` requirements.
`cargo-outdated` reports stale direct dependencies without forcing a manifest
change.
`cargo-deny` checks security advisories, licenses, and graph quality against
the configuration in `deny.toml`.
`cargo-machete` detects unused direct dependencies and is run as part of
`make deps-audit`.
`cargo-semver-checks` verifies that library crate APIs remain semver-compatible
after an upgrade and is run as part of `make deps-upgrade`.

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
cargo nextest run
cargo test --all-targets
bash scripts/check_forbidden_names.sh
```

To raise all compatible requirements in one pass, omit the package arguments:

```bash
make deps-plan
make deps-upgrade
```

For a deliberate semver-major review pass, pass the `--incompatible allow`
option through `ARGS`:

```bash
make deps-plan ARGS='--incompatible allow'
make deps-upgrade ARGS='--incompatible allow'
```

Scoped to one category:

```bash
make deps-deny ARGS="advisories"
make deps-deny ARGS="licenses"
```

## Unmaintained crate policy

`deny.toml` uses `unmaintained = "all"` to check the full dependency graph,
not just direct workspace dependencies. If `cargo deny check` fails on an
unmaintained transitive crate, add an entry to `[advisories].ignore` with a
`reason` field before merging (see the exception format below).

## Security advisory exceptions

Advisories are tracked in `deny.toml` under `[advisories].ignore`. Every entry
MUST include a `reason` field that records:

- The crate name and version affected
- The dependency path (which direct dep pulls it in)
- Why the advisory does not affect this codebase
- The expected resolution (upstream fix / replacement crate / version bump)

Suggested pattern:

```toml
[[advisories.ignore]]
id = "RUSTSEC-YYYY-NNNN"
reason = "fxhash is unmaintained; pulled in via starlark; no fixed release yet"
```

## Upgrade seams

The source of truth is the root `Cargo.toml`
`[workspace.metadata.upgrade-seams]` and `[workspace.metadata.upgrade-notes]`
tables. Cargo ignores those entries; they exist for maintainers and automation
so future bumps start from one manifest and one reviewed file map.

- `ratatui` and `crossterm`: start in `src/ui/tui.rs`, then adjust the small
  TUI adapter set listed in the metadata table (`src/tui_handle.rs`,
  `src/tui_frontend.rs`, `src/ui/editor/mod.rs`, and nearby UI call sites).
- `async-compression`: keep codec and output-cap changes localized to
  `src/net/compression.rs`.
- `jsonwebtoken`: keep JWT claim parsing and signature helper changes
  localized to `src/net/jwt.rs`.
- `oauth2`: keep PKCE, authorization URL wiring, and the custom async token
  exchange adapter localized to `src/net/oauth.rs`.
- `hickory-resolver`: keep DoH resolver and dual-stack lookup configuration
  localized to `src/net/dns.rs`.
- `rmcp`: stays localized in `src/mcp.rs`.
- `http`: start in `src/http_facade.rs`, then adjust the server and MCP files
  listed in the metadata table.
- `reqwest`: start in `src/api/client/mod.rs`, `src/net/http_client.rs`, and
  the existing server seams, then check `src/net/proxy.rs` for proxy URL
  formatting or feature-surface changes.
- `tokio`: start in `src/runtime/tokio.rs` for production runtime helpers. Do
  not try to wrap `#[tokio::test]` or similar proc-macro sites; leaving those
  direct is clearer and matches common Rust practice.
- `arboard`: stays localized in `src/clipboard.rs` and its single command
  handler call site.
- `serde`: do not add a facade. `derive` and `#[serde(...)]` attributes are
  compile-time annotations tied to the types that own them, so keeping them
  local is the lower-churn choice.
- `rust`: when the MSRV moves, update `package.rust-version` and CI toolchain
  declarations together.
- `quick-xml`, `tree-sitter`, and Markdown rendering crates keep using the
  existing focused seams: `src/state/conversation/tool_call_parser.rs`,
  `src/tools/index.rs`, and `src/ui/render/markdown.rs`.

The goal is not zero source edits. The goal is to keep upgrade work inside a
declared set of seam files instead of re-touching unrelated call sites across
the tree.

## Investigating impact

- `cargo tree -i <crate>` shows why one crate is in the graph and which direct
  dependency is pulling it in.
- `cargo tree -d` is useful for investigation, but it is not a hard gate in
  this repository because some ecosystems legitimately pull parallel transitive
  versions.
- `cargo metadata --no-deps --format-version 1` is the fastest way to inspect
  which workspace package owns a direct dependency.
- `cargo deny check bans` checks for wildcard requirements and duplicate-version
  entries without the full advisory database fetch.

## Future: automated PR creation

When the upgrade workflow matures and all active seams are well-tested, the
`scripts/upgrade-deps.sh apply` command can be wrapped in a GitHub Actions
scheduled workflow that opens a draft PR. The design prerequisite is that CI
passes on the manifest-only change for crates whose seam files have no API
churn in the release. The current Makefile targets are already structured for
this: `make deps-deny && make deps-upgrade && make gate` is a self-contained
pipeline that could run unattended.
