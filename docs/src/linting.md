# Lint Policy

This repository defines all workspace-wide lint rules in one place and
members inherit them automatically. The goal is the same as
[dependency centralization](dependency-upgrades.md): one file to edit,
consistent enforcement everywhere.

## How it works

`[workspace.lints]` in the root `Cargo.toml` (stable since Rust 1.74,
[RFC 3389](https://rust-lang.github.io/rfcs/3389-checklist-lint.html)) holds
the shared rule set. Each member crate opts in with a single stanza:

```toml
# in any crates/*/Cargo.toml
[lints]
workspace = true
```

Running `cargo clippy --all-targets -- -D warnings` (or `make lint`) applies
the workspace rules to every crate in one pass. CI enforces the same command
so local and remote results are identical.

## Current rules

| Table | Lint | Level | Reason |
| :--- | :--- | :--- | :--- |
| `[workspace.lints.rust]` | `unsafe_code` | `warn` | New `unsafe` blocks must carry a `// SAFETY:` comment; existing sites carry `#[allow(unsafe_code)]` with rationale. |
| `[workspace.lints.clippy]` | `incompatible_msrv` | `allow` | Suppresses false positives when a Clippy lint fires for an API that already exists on the declared MSRV. Remove when `rust-version` advances past the gated API. |

### MSRV-aware Clippy

`clippy.toml` at the workspace root sets `msrv = "1.91"`, matching
`workspace.package.rust-version`. This causes `clippy::msrv` lints to fire
locally on APIs that were stabilised after 1.91, giving early feedback
before CI.

## Unsafe code policy

`unsafe_code = "warn"` is the workspace default. The rule is:

1. **New unsafe** — add a `// SAFETY:` comment explaining the invariant that
   makes the block sound. CI will catch the warning (`-D warnings`).
2. **Existing unsafe** — annotate the enclosing function or `impl` item with
   `#[allow(unsafe_code)]` and a short rationale. Sites added so far:

   | File | Reason |
   | :--- | :--- |
   | `src/tools/search.rs` `mmap_read_file` | Memory-mapped read; no mutable alias during `Mmap` lifetime. |
   | `src/test_support.rs` `EnvLockGuard::set_var/remove_var` | `ENV_LOCK` serialises all callers. |
   | `src/test_support.rs` `EnvRestore::drop` | `EnvRestore` cannot outlive its `EnvLockGuard`. |
   | `src/bin/vex/tests.rs` (module level) | All `unsafe` env-var calls in this test module go through `ENV_LOCK`. |
   | `tests/integration_test.rs` (module level) | All `unsafe` env-var calls go through a module-local `ENV_LOCK`. |
   | `tests/disk_policy_tests.rs` (module level) | All `unsafe` env-var calls go through a module-local `ENV_LOCK`. |

## Progressive adoption guide

Adding a new lint or enabling a lint group follows this workflow:

```bash
# 1. Add the lint at "warn" level in root Cargo.toml [workspace.lints.*].
# 2. Run lint locally to see current violations.
make lint 2>&1 | grep "^warning"

# 3. Fix violations or add targeted #[allow(...)] with a rationale comment.
# 4. Re-run to confirm clean.
make lint

# 5. Commit the Cargo.toml lint addition + any source fixes together so the
#    PR diff is self-contained (no separate "fix lint" commits needed).
```

### Why not add all Clippy groups at once?

CI runs `cargo clippy --all-targets -- -D warnings`. Enabling a broad lint
group (e.g. `pedantic`) as `"warn"` is equivalent to `"deny"` under `-D
warnings`. Without first auditing the group for current violations, a single
`"warn"` entry can block dozens of unrelated PRs.

The recommended path: enable one lint at a time (or one carefully reviewed
subset), fix violations in the same commit, and document the rationale in
this file.

### Suggested next lints

These lints are commonly enabled in production Rust CLIs and are likely clean
or low-violation in this codebase. Each should be audited with `make lint`
before merging:

```toml
[workspace.lints.clippy]
cloned_instead_of_copied   = "warn"   # performance: prefer Copy over Clone for trivial types
inefficient_to_string      = "warn"   # performance: avoid unnecessary allocations
manual_string_new          = "warn"   # style: String::new() over "".to_string()
semicolon_if_nothing_returned = "warn" # style: explicit ; on statement-returning blocks
```
