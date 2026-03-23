# Architecture Overview

VexCoder currently has two operator-facing surfaces in the source tree:

- the interactive CLI UI started by `src/bin/vex.rs`
- the non-interactive batch runner in `src/batch_mode.rs`

Most interactive application coordination still lives in `src/app.rs`. The runtime core lives under `src/runtime/`, including context assembly, the edit loop, command execution, validation, and task state.

## Current code layout

- `src/bin/vex.rs` parses CLI arguments, loads config, and routes startup into the interactive UI, batch mode, export, compatibility helpers, and other CLI paths.
- `src/app.rs` owns the current interactive command surface, transcript state, approval prompts, and runtime-facing coordination for the full-screen TUI.
- `src/ui/draw/` owns the direct ANSI task-surface renderer used while the fullscreen task surface is active; it draws a human-readable header, optional changed-files row, adaptive timeline, prompt-anchored transcript area, and larger multiline composer without allocating a ratatui frame buffer per update. The fullscreen composer now shows live focus state and character count in its header, keeps wrapped `/command`, `@path`, and pasted prompt text editable in place, and turns `@path` suggestions into an interactive picker: `Up` / `Down` cycle matching files, `Enter` inserts the selected workspace-relative path, and `Esc` dismisses the picker so the raw mention token can still be submitted unchanged. Outside picker mode, the composer still supports visual-row `Up` / `Down` / `Home` / `End` navigation instead of forcing the operator out of task mode, while terminal selection and copy gestures stay with the terminal because the UI does not enable mouse capture. While timeline follow mode is active, the output pane stays on the accumulated transcript so each new server response appends to the existing scrollback instead of replacing it. Manual timeline navigation can still switch that pane into per-step inspector detail when the operator wants to inspect a tool call.
- `src/app/model_update.rs` pushes a verb-first one-liner into the transcript as each tool result arrives (e.g. "Searched …", "Read …", "Edited …") so the operator sees immediate progress instead of a blank screen while the model produces its response text.
- `src/batch_mode.rs` runs the same runtime headlessly for `vex exec` and writes JSONL or text output.
- `src/runtime/` contains the reusable runtime machinery: context assembly, the edit loop, command and sandbox plumbing, project instructions, task state, and validation.

## Streaming protocol coverage

The shared SSE parser in `src/api/stream.rs` and the normalized type surface in
`src/types/api_types.rs` preserve documented streaming values from both
`messages-v1` and `chat-compat` backends.

- heartbeats and structured stream errors
- text, input-json, thinking, and signature deltas
- citations, server-tool blocks, and web-search tool results
- normalized usage totals plus cache, geography, and token-detail metadata
- chat-compat chunk metadata such as service tier, system fingerprint, refusal
  text, logprobs, choice indexes, and tool-call type

Not every metadata field is rendered in the interactive transcript today, but
the parser keeps those values in the normalized event surface instead of
dropping them during protocol conversion.

## Ongoing boundary work

The long-term architecture work is tracked in the ADR set under `adr/`.

- ADR-025 defines the canonical machine-readable runtime request and event contract.
- ADR-026 defines the proposed `LocalApiServer` transport binding over that contract.
- ADR-028 is now active in the current tree: the facade helpers live under `src/app/`, the localhost `/v1/messages` protocol-routing fix is in place, and the full-screen task surface now keeps live orchestration rows visible while follow-up work continues to shrink `src/app.rs` behind the facade boundary.
- ADR-030 defines the runtime control-flow rule: provider events normalize into canonical runtime events, task state owns execution truth, and the orchestrator decides whether the task continues or stops.
- ADR-031 extends the active operator surface with timeline selection, stable step identity, explicit approved/running/completed lifecycle rendering, adaptive timeline sizing, prompt-anchored transcript scrolling, a larger multiline composer, direct ANSI task rendering during orchestration, and keyboard navigation for a terminal-height-scaled timeline window.

That means the current `src/app.rs`-centric layout is still the live implementation, but it is not intended to be the permanent shape for machine-readable runtime access or local server transports.
