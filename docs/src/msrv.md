# MSRV Policy

MSRV (Minimum Supported Rust Version) is the oldest `rustc` release this
codebase is tested and guaranteed to compile with.

## Declared value

`workspace.package.rust-version` in the root `Cargo.toml` is the single
source of truth:

```toml
[workspace.package]
rust-version = "1.91"
```

Both member crates inherit it with `rust-version.workspace = true`. The
`clippy.toml` `msrv` field mirrors this value so Clippy lints fire locally
on MSRV-incompatible API usage.

## Why MSRV matters

Dependency upgrades can silently raise the effective MSRV of the workspace if
a new crate version requires a newer compiler. The resolver 3 / MSRV-aware
resolver (enabled by `resolver = "3"` in the workspace since Rust 1.84,
defaulted for `edition = "2024"`) prefers crate versions compatible with the
declared `rust-version` instead of always selecting the newest release.

See the [Rust 2024 Edition Guide §cargo-resolver][edition-guide] for the full
specification.

[edition-guide]: https://doc.rust-lang.org/edition-guide/rust-2024/cargo-resolver.html

## Bumping the MSRV

1. Update `workspace.package.rust-version` in the root `Cargo.toml`.
2. Update the `msrv` field in `clippy.toml` to match.
3. Update the CI toolchain wherever a specific Rust version is pinned (search
   `ci.yml`, `deny.yml`, and any other workflow files for `toolchain` or
   `rustup toolchain install`).
4. Note the change in the `[workspace.metadata.upgrade-notes]` `rust` entry
   as a reminder to update CI together.

Per the upgrade notes:
> "Update package.rust-version and the CI toolchain declarations together
> when the MSRV changes."

## MSRV CI gate (future work)

A dedicated MSRV CI job (`cargo +<MSRV> check --workspace`) should be added
to `ci.yml` to catch transitive MSRV regressions before they reach main.
This is tracked as item C-1 in the workspace-hygiene improvement backlog.
Until that job exists, local verification is:

```bash
rustup toolchain install 1.91
cargo +1.91 check --workspace --all-targets
```

## Resolver version

The workspace uses `resolver = "3"` (the default for the 2024 edition).
Resolver 3 activates `incompatible-rust-versions = "fallback"`, which causes
Cargo to prefer dependency versions compatible with `rust-version` when
selecting from a version range. This reduces the risk of a routine
`cargo update` silently pulling in a crate version that requires a newer
compiler than the declared MSRV.

Note: resolver 3 requires `rustc` ≥ 1.84. Because the workspace `rust-version`
is 1.91, this requirement is already satisfied.
