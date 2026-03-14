# Architecture Overview

VexCoder currently has two operator-facing surfaces in the source tree:

- the interactive terminal UI started by `src/bin/vex.rs`
- the non-interactive batch runner in `src/batch_mode.rs`

Most interactive application coordination still lives in `src/app.rs`. The runtime core lives under `src/runtime/`, including context assembly, the edit loop, command execution, validation, and task state.

## Current code layout

- `src/bin/vex.rs` parses CLI arguments, loads config, and routes startup into the interactive UI, batch mode, export, migration, and other CLI paths.
- `src/app.rs` owns the current interactive command surface, transcript state, approval prompts, and runtime-facing coordination for the full-screen TUI.
- `src/batch_mode.rs` runs the same runtime headlessly for `vex exec` and writes JSONL or text output.
- `src/runtime/` contains the reusable runtime machinery: context assembly, the edit loop, command and sandbox plumbing, project instructions, task state, and validation.

## Ongoing boundary work

The long-term architecture work is tracked in the ADR set under `adr/`.

- ADR-025 defines the canonical machine-readable runtime request and event contract.
- ADR-026 defines the proposed `LocalApiServer` transport binding over that contract.
- ADR-028 defines the next refactor boundary: shared application semantics should move out of `src/app.rs` into a smaller application facade, while future server transport code should live in separate `src/server/` modules.

That means the current `src/app.rs`-centric layout is still the live implementation, but it is not intended to be the permanent shape for machine-readable runtime access or local server transports.