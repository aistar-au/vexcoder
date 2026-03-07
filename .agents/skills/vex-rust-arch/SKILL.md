---
name: vex-rust-arch
description: >
  Repo-local Rust architecture pattern dictionary for aistar-au/vexcoder.
  Load after baseline skills when implementing or reviewing Rust changes,
  including config loading, serde/TOML validation, integration test setup,
  and ADR-024 gap implementation.
---

# Vex Rust Arch

Use this skill as a repository-specific Rust pattern dictionary.
It supplements, but does not replace, ADR and source verification.

## When to load

- Changes in `src/**/*.rs`
- Runtime architecture edits (`src/runtime/**`, `src/app.rs`, `src/state/**`, `src/tools/**`)
- ADR-024 gap implementation or dispatch verification
- Any change touching config loading, TOML parsing, or integration test helpers

## Pattern dictionary

### Structural boundaries

- Runtime boundary pattern:
  `src/runtime/` owns orchestration and policy wiring.
  UI crates must not leak into runtime.
- UI orchestration pattern:
  `src/app.rs` is the command and mode surface.
  Keep routing stable and explicit.
- Tool operator pattern:
  `src/tools/operator.rs` enforces workspace-root confinement for tool file access.
- State pattern:
  `src/state/**` owns conversation and task-state persistence contracts.
- Diff and approval pattern:
  approval and edit flows stay deterministic; avoid alternate hidden routing paths.

### Config / TOML layer pattern (ADR-024 PA-01)

Use when implementing layered configuration loading with strict TOML validation.

- Represent each layer as a struct with all fields `Option<T>`. This allows partial
  merging where each layer fills only the gaps the layers above it left open.
- Apply `#[serde(deny_unknown_fields)]` to the layer struct, not to the final resolved
  `Config`. This catches typos at parse time with file-path context.
- Double-parse when a field must be explicitly prohibited: first parse to `toml::Value`
  to detect and reject the forbidden field with a user-facing message naming the file,
  then parse again as the typed struct. Do not use `#[serde(skip)]` on forbidden
  fields — serde will silently accept them without error.
- For ancestor-walk config discovery, walk from the actual `cwd` argument, never from
  the resolved `working_dir`. Using `working_dir` to find the config that defines
  `working_dir` creates a bootstrap cycle.
- Invalid enum string values are startup failures with file-path context; they must not
  fall through to a default. Wrap `toml::de::Error` with `anyhow::Context` that names
  the file.
- Error messages for rejected enum strings must list all accepted aliases, not just the
  canonical form. Omitting aliases causes repeated "try again" debugging cycles.

### Serde / TOML error quality

- Include the source file path in every TOML parse error so operators can locate the
  bad config without guessing.
- Enum validation errors must enumerate all accepted values in the message. A single
  canonical form in the error leaves users guessing at alternatives.

### Test helper pattern

- Expose `pub fn load_for_tests(cwd: &Path, user: Option<&Path>, system: Option<&Path>)
  -> Result<Self>` in the main module body (not under `#[cfg(test)]`) so the `tests/`
  integration crate can call it. Integration tests live in a separate compilation unit
  and cannot access `#[cfg(test)]`-gated items in the library.
- Use `Option<&Path>` for injectable fixture paths so tests can pass `None` to get the
  OS default for that layer when the layer is not under test.

### ENV_LOCK pattern for integration tests

`tokio::sync::Mutex` does not implement the `std::sync::Mutex` API. The two common
mistakes are using `.lock().unwrap()` (which will not compile) and declaring the lock
in unit-test modules (which integration tests cannot access).

Correct pattern for integration tests:

```rust
mod test_support {
    pub static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
}

// In each test:
let _lock = crate::test_support::ENV_LOCK.blocking_lock();
```

Rules:
- Use `.blocking_lock()`, not `.lock().unwrap()`.
- Declare `ENV_LOCK` in each crate that needs it. Each integration test binary declares
  its own; they do not share across crate boundaries.
- Set env vars only while the lock is held. Remove them unconditionally after assertions
  — a panic before `remove_var` will leave state that poisons subsequent tests.

## Verification contract

- Run `make check-arch` for boundary checks.
- Run `cargo test --all-targets` for behavior verification.
- For source assertions in review text, use SHA-pinned GitHub verification.
- Run `cargo fmt --check` before finalizing any `*.rs` patch (see Hard Rule 24 in
  `vex-remote-contract`).

## Non-goals

- This skill does not define new boundaries outside ADRs.
- This skill does not authorize live web lookups for architecture claims.
- This skill does not override `vex-local-bash` or `vex-remote-contract`.
