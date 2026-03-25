# Architecture Overview

VexCoder currently has two operator-facing surfaces in the source tree:

- the interactive CLI UI started by `src/bin/vex.rs`
- the non-interactive batch runner in `src/batch_mode.rs`

Most interactive application coordination still lives in `src/app.rs`. The runtime core lives under `src/runtime/`, including context assembly, the edit loop, command execution, validation, and task state.

## Current code layout

- `src/bin/vex.rs` parses CLI arguments, loads config, and routes startup into the interactive UI, batch mode, export, compatibility helpers, and other CLI paths.
- `src/app.rs` owns the current interactive command surface, transcript state, approval prompts, and runtime-facing coordination for the full-screen TUI.
- `src/ui/draw/` owns the direct ANSI task-surface renderer used while the fullscreen task surface is active; it draws a human-readable header, optional changed-files row, adaptive timeline, prompt-anchored transcript area, and larger multiline composer without allocating a ratatui frame buffer per update. The fullscreen composer now shows live focus state and character count in its header, keeps wrapped `/command`, `@path`, and pasted prompt text editable in place, and turns `@path` suggestions into a repo-wide interactive picker: `Up` / `Down` traverse ranked matches across the full workspace tree, `Enter` inserts the selected workspace-relative path, and `Esc` dismisses the picker so the raw mention token can still be submitted unchanged. The picker keeps a bounded ranked candidate set per keystroke so large workspaces do not pay a full-tree sort cost on every input edit. Free-form slash commands such as `/edit`, `/plan`, and `/review` consume those selected `@path` mentions as inline context before the model turn starts, while `/explain` treats `@path` as the requested file target. `/edit` and `/fix` also seed task-scoped edit grants (`write-file`, `apply-patch`, `run-command`) so the mutation workflow stays live after the slash command starts without downgrading broader session grants. Outside picker mode, the composer still supports visual-row `Up` / `Down` / `Home` / `End` navigation instead of forcing the operator out of task mode, while terminal selection and copy gestures stay with the terminal because the UI does not enable mouse capture. While timeline follow mode is active, the output pane stays on the accumulated transcript so each new server response appends to the existing scrollback instead of replacing it. Manual timeline navigation can still switch that pane into per-step inspector detail when the operator wants to inspect a tool call.
- `src/app/model_update.rs` pushes a verb-first one-liner into the transcript as each tool result arrives (e.g. "Searched …", "Read …", "Edited …") so the operator sees immediate progress instead of a blank screen while the model produces its response text.
- `src/batch_mode.rs` runs the same runtime headlessly for `vex exec` and writes JSONL or text output.
- `src/runtime/` contains the reusable runtime machinery: context assembly, the edit loop, command and sandbox plumbing, project instructions, task state, and validation.
- `src/state/conversation/` owns the conversation loop safeguards that sit above raw tool execution. Alongside the existing read-only and mutating-tool guards, it now short-circuits malformed `read_file` calls with missing paths and asks for a concrete file target or a repo-overview flow (`list_files` / `codebase_search`) instead of replaying the same raw tool error, including mixed parallel read-only rounds where a good `list_files` call and a malformed `read_file` arrive together. Write guards enforce `VEX_DIFF_PREFERRED_ABOVE_LINES` (warning) and `VEX_WRITE_FILE_MAX_LINES` (rejection) thresholds, steering the model toward `apply_patch` or `edit_file` for large files. Conversation history older than `VEX_HISTORY_KEEP_TURNS` turns (default 10) is condensed: tool results are truncated to the first 5 lines plus a line-count indicator to stay within the context budget.
- `src/server/` owns the ADR-026 transport plumbing: HTTP routing and auth middleware (`http.rs`), SSE response framing (`sse.rs`), Unix socket binding (`socket.rs`), request handlers (`handlers.rs`), TLS helpers and config resolution (`util.rs`). Transport code reaches the runtime only through facade entrypoints in `src/app/`.
- `src/local_api.rs` contains the `LocalApiMode` (RuntimeMode) and `LocalApiFrontend` (FrontendAdapter) that bridge the local API surface to the runtime engine.
- `src/tools/search.rs` implements the `codebase_search` tool using a Tree-sitter-based structural index for Rust source files. The index extracts functions, structs, enums, impls, traits, modules, constants, and type aliases, and ranks results by exact name match, substring match, parent-scope match, and content keyword match.
- `src/tools/semantic.rs` manages the optional semantic vector index persisted at `.vex/index/`. When `VEX_EMBEDDING_PROVIDER` is configured, chunks are embedded at logical boundaries and results are reranked by cosine similarity merged with structural scores.
- `src/tools/embed.rs` provides the embedding client for the `/v1/embeddings`-compatible endpoint used by semantic search.

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
- ADR-028 is now active in the current tree: the facade helpers live under `src/app/`, transport code has been extracted from `src/local_api.rs` into `src/server/` submodules (`http.rs`, `sse.rs`, `socket.rs`, `handlers.rs`, `util.rs`), and dependency-direction enforcement tests verify inward-only import rules across all layers.
- ADR-029 is now accepted: the stream parser covers all documented SSE event types (error envelopes, heartbeats, thinking/signature deltas, citations, server-tool blocks, web-search results, cache/geo/detail metadata) and TaskState persists plan, session notes, context compaction records, and cache usage stats for multi-agent handoff. ADR-029 is a declared dependency of ADR-030 and a prerequisite for full invariant compliance — `StreamEvent::Error` lets orchestrating agents detect sub-agent stream failures, and the TaskState extensions are the handoff payload that lets an orchestrator reconstruct a sub-agent's context on resume.
- ADR-030 is now accepted with an explicit six-point verification suite: provider events normalize into canonical runtime events, task state owns execution truth, the orchestrator decides whether the task continues or stops, and task handoff or resume consumers depend on that same runtime-owned control flow. ADR-030 is also load-bearing for multi-agent orchestration: Invariants 1, 4, and 5 are the semantic correctness guarantees that make agent handoffs coherent. Without these invariants proven end-to-end, multi-agent orchestration has undefined behaviour at handoff points.
- ADR-031 extends the active operator surface with timeline selection, stable step identity, explicit approved/running/completed lifecycle rendering, adaptive timeline sizing, prompt-anchored transcript scrolling, a larger multiline composer, direct ANSI task rendering during orchestration, and keyboard navigation for a terminal-height-scaled timeline window. Each pending tool call carries a stable `step_id` and compact input preview. The timeline derives pending rows as `AwaitingApproval`, `Approved`, or `Running` from canonical state, and the `Approved` state is tracked for manual approvals, session auto-approvals, and capability-grant auto-approvals.

- ADR-032 adds prompt-area interactivity: interactive `/` slash command picker and `@path` file picker with `Up`/`Down`/`Enter`/`Esc` navigation and hierarchical directory drill-down, `!command` shell execution, pasted-block handling, and a context guard that limits project-instructions and notes token budgets.
- ADR-033 introduces the hybrid retrieval context architecture: a `codebase_search` tool (Phase 1) backed by structural keyword indexing, optional semantic vector search via an external embedding endpoint (Phase 2), write guards that steer `write_file` toward `apply_patch`/`edit_file` for large files (Phase 3), and history condensing that compresses older tool results to stay within the context budget (Phase 4).

The transport layer (`src/server/`) now reaches the runtime exclusively through the application facade (`src/app/`), and `src/local_api.rs` retains only the `LocalApiMode` / `LocalApiFrontend` runtime-mode bridge types.
