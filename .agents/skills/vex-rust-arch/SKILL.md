---
name: vex-rust-arch
description: >
  Repo-local Rust architecture pattern dictionary for aistar-au/vexcoder.
  Load after baseline skills when implementing or reviewing Rust changes.
---

# Vex Rust Arch

Use this skill as a repository-specific Rust pattern dictionary.
It supplements, but does not replace, ADR and source verification.

## When to load

- Changes in `src/**/*.rs`
- Runtime architecture edits (`src/runtime/**`, `src/app.rs`, `src/state/**`, `src/tools/**`)
- ADR-024 gap implementation or dispatch verification

## Pattern dictionary

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

## Verification contract

- Run `make check-arch` for boundary checks.
- Run `cargo test --all-targets` for behavior verification.
- For source assertions in review text, use SHA-pinned GitHub verification.

## Non-goals

- This skill does not define new boundaries outside ADRs.
- This skill does not authorize live web lookups for architecture claims.
- This skill does not override `vex-local-bash` or `vex-remote-contract`.
