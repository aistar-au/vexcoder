# Architecture Overview

VexCoder currently has two operator-facing surfaces in the source tree:

- the interactive CLI UI started by `src/bin/vex.rs`
- the non-interactive batch runner in `src/batch_mode.rs`

Most interactive application coordination is rooted at `src/app.rs` and its
split submodules under `src/app/` (for example `commands/`,
`slash_commands.rs`, and `layout.rs`). The runtime core is found under
`src/runtime/`, including context assembly, the edit loop, command execution,
validation, and task state.

## Current code layout

- `src/bin/vex.rs` parses CLI arguments, loads config, and routes startup into the interactive UI, batch mode, export, compatibility helpers, and other CLI paths.
- `src/app.rs` is the interactive application module root. The full-screen TUI command surface is found across `src/app.rs`, `src/app/commands/`, `src/app/slash_commands.rs`, and related helper modules under `src/app/`.
- `src/ui/draw/` owns the direct ANSI task-surface renderer used while the fullscreen task surface is active; it draws a full-height transcript body, multiline composer, and compact status bar without allocating a ratatui frame buffer per update. The rendering and infrastructure stack includes: `unicode-width` and `unicode-segmentation` for grapheme-aware display width calculations; `textwrap` for paragraph wrapping; `ansi-to-tui` for converting raw ANSI escape sequences into ratatui `Span`/`Line` structures; `arboard` for programmatic clipboard access via the `/copy` slash command; `pulldown-cmark` and `syntect` for the shared markdown rendering helpers used by the fallback renderer, with inline markdown styling active today and fenced-code highlighting handled inside the shared conversion module; `ratatui-macros` for the fallback renderer's line and span construction helpers; `diffy` for unified diff rendering in edit previews (replacing the earlier hand-rolled LCS implementation); `color-eyre` for structured panic hooks and pretty backtraces; `dirs` for cross-platform XDG/config directory resolution; `tracing-appender` for daily-rotated file logging when `RUST_LOG` is set; `indicatif` and `console` for progress spinners in headless batch mode; `ignore` and `globset` for gitignore-compliant workspace traversal and glob matching (replacing the earlier hand-rolled glob engine); `pathdiff` and `dunce` for cross-platform relative path computation; `chrono` for ISO 8601 timestamps in SSE event streams and all internal timestamp generation (replacing `SystemTime`/`UNIX_EPOCH`); `base64` for binary content encoding in exports; `indexmap` for ordered insertion-preserving maps used in streaming tool-call accumulation (`DerivedTurnState.pending_tool_calls`), ensuring tool calls are serialized in the order they were opened; `tower-http` for the `TraceLayer::new_for_http()` middleware wired into `build_http_router()`, providing structured request/response tracing for authorized and unauthorized HTTP requests. `crossterm` is configured with `bracketed-paste` (prevents input corruption on multi-line pastes; active in `src/terminal.rs`) and `event-stream` (async terminal event integration). `ratatui` enables `unstable-rendered-line-info`, `unstable-widget-ref`, and `unstable-backend-writer` for scroll-offset tracking, efficient widget updates, and backend-writer parity in the fallback renderer. Tool calls, waiting-state telemetry, and assistant responses stream into transcript paragraphs on that shared body instead of a dedicated visible timeline strip. The fullscreen composer now shows current focus state and character count in its header, auto-fits against the current display row and column budget, keeps wrapped `/command`, `@path`, and pasted prompt text editable in place, and turns `@path` suggestions into a repo-wide interactive picker: `Up` / `Down` traverse ranked matches across the full workspace tree, `Enter` inserts the selected workspace-relative path, and `Esc` dismisses the picker so the raw mention token can still be submitted unchanged. The picker keeps a bounded ranked candidate set per keystroke so large workspaces do not pay a full-tree sort cost on every input edit. Free-form slash commands such as `/edit`, `/plan`, and `/review` consume those selected `@path` mentions as inline context before the model turn starts, while `/explain` treats `@path` as the requested file target. `/edit` and `/fix` also seed task-scoped edit grants (`write-file`, `apply-patch`, `run-command`) so the mutation workflow remains active after the slash command starts without downgrading broader session grants. Outside picker mode, the composer still supports visual-row `Up` / `Down` / `Home` / `End` navigation instead of forcing the operator out of task mode, while cli selection and copy gestures stay with the cli because the UI does not enable mouse capture. While timeline follow mode is active, the output pane stays on the accumulated transcript so each new server response appends to the existing scrollback instead of replacing it. Manual timeline navigation can still switch that pane into per-step inspector detail, the footer shows whether the surface is following live output or browsing a selected step, and `Alt+End` returns the surface to live follow mode without restoring a dedicated activity strip. The `expand_rows_for_display` helper splits embedded newlines before word-wrapping each sub-line, so server responses containing literal `\n` sequences render as separate visual rows. `is_structural_transcript_row` now recognises bullet list items (`- `, `* `) and numbered list items (`1. `, `2) `) as structural, passing them through the draw path without word-wrap reflow. `format_compact_paragraph` preserves empty lines as paragraph separators instead of silently dropping them. Scroll-offset clamping in `apply_output_scroll_action` and `preserve_transcript_scroll_on_growth` now uses the expanded (word-wrapped) row count instead of the raw output row count, so the viewport range matches the draw path and all rows are reachable. The ratatui fallback renderer now windows output against that same expanded-row space, so wrapped paragraphs, multiline tool previews, and long diff hunks scroll the same way outside the direct ANSI path.
- `src/app/model_update.rs` pushes a verb-first one-liner into the transcript as each tool result arrives (e.g. "Searched …", "Read …", "Edited …") so the operator sees immediate progress instead of a blank screen while the model produces its response text. Consecutive completed read-only tools (`codebase_search`, `read_file`, `search`, `search_files`, `search_content`, `find_files`, `list_files`, `list_dir`, `list_directory`, `glob_files`, `git_status`, `git_diff`, `git_log`, `git_show`) now fold into a single `[tool]` paragraph regardless of tool name, keeping the transcript compact during multi-tool exploration sequences. Pending and completed `edit_file` rows also keep the structured multiline diff preview instead of collapsing the change into one JSON line, which preserves per-hunk evidence and add/remove color feedback in both renderers.
- `src/batch_mode.rs` runs the same runtime headlessly for `vex exec` and writes JSONL or text output.
- `src/runtime/` contains the reusable runtime machinery: context assembly, the edit loop, command and sandbox plumbing, project instructions, task state, and validation. The Phase 1 ADR-038 split adds `src/runtime/context_cache.rs` for bounded in-memory file-rollup reuse and `src/runtime/git_rollup.rs` for opt-in git status/diff capture, so automatic turn assembly no longer has to pay synchronous git overhead by default.
- `src/state/conversation/` owns the conversation loop safeguards that sit above raw tool execution. Alongside the existing read-only and mutating-tool guards, it now short-circuits malformed `read_file` calls with missing paths and asks for a concrete file target or a repo-overview flow (`list_files` / `codebase_search`) instead of replaying the same raw tool error, including mixed parallel read-only rounds where a good `list_files` call and a malformed `read_file` arrive together. Write guards enforce `VEX_DIFF_PREFERRED_ABOVE_LINES` (warning) and `VEX_WRITE_FILE_MAX_LINES` (rejection) thresholds, steering the model toward `apply_patch` or `edit_file` for large files. Conversation history older than `VEX_HISTORY_KEEP_TURNS` turns (default 10) is condensed: tool results are truncated to the first 5 lines plus a line-count indicator to stay within the context budget.
- `src/server/` owns the ADR-026 transport plumbing: HTTP routing and auth middleware (`http.rs`), SSE response framing (`sse.rs`), Unix socket binding (`socket.rs`), request handlers (`handlers/mod.rs`, `handlers/session.rs`), TLS helpers and config resolution (`util.rs`). Transport code reaches the runtime only through facade entrypoints in `src/app/`.
- `src/local_api.rs` contains the `LocalApiMode` (RuntimeMode) and `LocalApiFrontend` (FrontendAdapter) that bridge the local API surface to the runtime engine.
- `src/tools/search.rs` implements the `codebase_search` tool using a Tree-sitter-based structural index for Rust source files. The index extracts functions, structs, enums, impls, traits, modules, constants, and type aliases, and ranks results by exact name match, substring match, parent-scope match, and content keyword match.
- `src/tools/semantic.rs` manages the optional semantic vector index persisted at `.vex/index/`. When `VEX_EMBEDDING_PROVIDER` is configured, chunks are embedded at logical boundaries and results are reranked by cosine similarity merged with structural scores.
- `src/tools/embed.rs` provides the embedding client for the `/v1/embeddings`-compatible endpoint used by semantic search.
- `src/tools/workspace_explore.rs` provides the `list_dir` and `glob_files` tools for workspace exploration. Both are workspace-confined, `.gitignore`-aware, and bounded to prevent unbounded output.
- `src/tools/workspace_ignore.rs` implements `WorkspaceIgnore` on top of the `ignore` crate's gitignore matcher so that `search_files`, `list_dir`, `glob_files`, and `find_files` all skip ignored paths with gitignore-compatible directory semantics.

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

A `StreamTextNormaliser` layer at the `forward_conversation_update` boundary
intercepts embedded tool call markup (XML-like tags from local inference
servers) and converts them into structured `[tool]`/`[detail]` transcript
lines before they reach the TUI. This prevents raw SSE event data from leaking
to the display and ensures all tool invocations render as paragraph blocks in
the scrolling transcript pane. The local API handoff in
`src/runtime/json_handoff.rs` and `src/local_api.rs` preserves those transcript
rows plus transcript block start/delta/complete updates as canonical
`RuntimeEnvelope` JSON events, so downstream clients can stay transcript-first
over SSE without reparsing a flattened assistant text stream. The
normaliser buffers chunk-split `<tool_call>`, `<function=...>`, and
`<parameter=...>` fragments until they are complete enough to classify,
so transcript-first consumers follow the backend's JSON delta stream
without showing raw wrapper or partial tag text when the server breaks
markup across arbitrary chunk boundaries.

The live parser path for interactive turns remains the shared stream parser,
the tool-call parser selected by the conversation loop, and the
`StreamTextNormaliser` boundary that converts malformed inline tool markup into
transcript-safe rows. The `structured_parser` module is present in tree as an
optional framework and does not replace the live runtime parser path unless the
ADR-043 adoption gates are satisfied.

A delta-native rendering foundation (`src/state/transcript_delta.rs`) provides
structured `TranscriptDelta` and `DeltaAccumulator` types that track streaming
blocks with bounded suffix deduplication — O(new_text) rather than
O(total_content) — and expose pending deltas for the draw layer. Delta
accumulators are keyed by block index in TuiMode and run in parallel with the
existing prefix-marker line path so both strategies coexist. Individual scaffolding methods (`apply_transcript_delta`,
`consume_transcript_deltas`, `content`, `set_block_kind`) carry targeted
`#[allow(unused)]` annotations until the live draw path switchover activates
them; the remaining foundation methods are now used by the active code paths.
`TaskDraw::apply_transcript_delta()` and `format_compact_paragraph()` in the
draw module provide the direct delta-to-display path that bypasses the
`[tool]`/`[detail]`/`[evidence]` prefix-marker chain (ADR-041 D5–D7).

The runtime envelope schema (`schemas/runtime_envelope_v1.json`) accepts tool
names matching `[a-z][a-z0-9_-]*` and MCP-namespaced tools
(`mcp.<provider>.<tool>`), covering all built-in and external tool
registrations.

## Crate design boundaries -- text processing

VexCoder uses several crates that touch text at different abstraction layers.
Each crate occupies a distinct role with no overlap.  The boundary rule is:
**never use a search/indexing crate for internal string surgery, and never use
a string-surgery crate for file-content search or structural parsing.**

### Non-overlapping crate roles

| Crate | Role | Scope | NOT used for |
|-------|------|-------|-------------|
| `aho-corasick` | Multi-pattern **literal** matching | File content search, keyword extraction from source text | Git output parsing, secret redaction |
| `regex-lite` | Lightweight **internal string surgery** | Git output parsing, secret redaction, rate-limit extraction, format validation | Code search, RAG, semantic indexing, codebase search |
| `tree-sitter` | **Structural AST** indexing | Language-aware parsing of source files into syntax trees | String surgery, log parsing, redaction |
| `globset` / `ignore` | **Filesystem traversal** | `.gitignore`-aware path matching and directory walking | File content search, string processing |
| `quick-xml` | **XML tool-call** parsing | Structured extraction of `<function=...>` / `<parameter=...>` tags from model output | Git parsing, log analysis |
| `indexmap` | **Ordered** insertion-preserving maps | Streaming tool-call accumulation preserving insertion order | Search indexing, text processing |
| `tower-http` | **HTTP middleware** | Request/response tracing for the local API server | Application logic, text processing |

### regex-lite -- ASCII-only internal string surgery

`regex-lite` is the **only** regex crate in the dependency tree.  All patterns
are ASCII-only (`\d` = `[0-9]`, `\w` = `[0-9A-Za-z_]`).  Non-ASCII characters
are not supported in regex-lite patterns.  This is intentional -- vexcoder's
regex-lite usage exclusively targets machine-readable ASCII output from git,
HTTP headers, and API responses.

Conventional use cases DISTINCT from RAG/semantic search/codebase_search:

- **Parsing structured output** from external tools (git status, git diff, git apply, git log)
- **Extracting known fields** from semi-structured strings (retry delays, durations)
- **Sanitizing/redacting sensitive data** from logs, transcripts, and telemetry
- **Format validation** (API key formats, token patterns, connection strings)

None of these overlap with codebase search, RAG, or semantic indexing.

The `regex-lite` modules live under `src/runtime/` as three focused files:

- **`git_parse.rs`** -- Structured parsing of `git status --porcelain`, `git diff --stat`, `git diff --name-status`, `git log --oneline`, and `git apply` output into typed enums and structs.  Patterns compile once via `OnceLock<regex_lite::Regex>` and are reused across calls.
- **`secrets.rs`** -- Output redaction for OpenAI keys (`sk-...`), AWS access keys (`AKIA...`), GitHub PATs (`ghp_`/`gho_`/`ghu_`/`ghs_`/`ghr_`), PEM private key headers, bearer tokens, connection strings with embedded credentials, and generic secret assignments.  Wired into `sanitize_assistant_text` so secrets never leak into the transcript or logs.
- **`rate_limit.rs`** -- Extracts retry delay hints from `Retry-After` header values and error response body text ("try again in N seconds").  The header path is wired into `map_api_status_error` in the API client with fallback to body text for 429 detection.

Design rationale: `regex-lite` was chosen over the full `regex` crate because
(a) vexcoder does not allow non-ASCII characters in these internal patterns,
(b) the ~94 KB binary size overhead vs ~373 KB for full `regex` is meaningful
for a CLI binary, and (c) the `O(m*n)` execution guarantee is the same.

### Stream parser -- no regex

The stream parser (`src/api/stream.rs`) and text normaliser
(`src/api/stream/text_normaliser.rs`) handle SSE framing, JSON delta
parsing, and embedded XML-like tool call markup using zero-regex string
scanning (`starts_with`, `contains`, manual index arithmetic).  `quick-xml`
handles structured XML extraction.  regex-lite is not used in the streaming
path.

### Full git parsing stack

The git parsing stack is the foundation of vexcoder's value as a CLI tool
working with git repos.  The following git output formats are parsed:

| Command | Parser | Output type |
|---------|--------|-------------|
| `git status --porcelain` | `parse_git_status` | `ParsedGitStatus` with per-file status entries |
| `git diff --stat` | `parse_diff_stat` | `ParsedDiffStat` with per-file changes and summary |
| `git diff --name-status` | `parse_name_status` | `ParsedNameStatus` with status chars and rename detection |
| `git log --oneline` | `parse_git_log_oneline` | `ParsedGitLog` with hash + subject entries |
| `git apply` (stdout+stderr) | `parse_git_apply` | `ParsedGitApply` with outcome classification per line |

All parsers live in `src/runtime/git_parse.rs` and are re-exported from
`src/runtime.rs`.  `git_rollup.rs` orchestrates git command execution with
timeout and cancellation support, using `parse_git_status` to produce
structured rollups for context assembly.

### Secret redaction -- always on

Secret redaction runs on every assistant text output through
`sanitize_assistant_text` in `src/runtime/policy.rs`.  The following
patterns are detected and replaced with `[REDACTED]`:

- OpenAI API keys (`sk-` prefix, 20+ chars)
- AWS access key IDs (`AKIA` prefix, 16 uppercase alphanumeric)
- GitHub personal access tokens (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_` prefixes, 36+ chars)
- PEM private key headers (`-----BEGIN ... PRIVATE KEY-----`)
- Bearer tokens (preserving the `Bearer ` prefix)
- Connection strings with embedded passwords (`protocol://user:password@host`)
- Generic secret assignments (`API_KEY=...`, `token: "..."`, etc.)

## Ongoing boundary work

The long-term architecture work is tracked in the ADR set under `adr/`.

- ADR-025 defines the canonical machine-readable runtime request and event contract.
- ADR-026 defines the proposed `LocalApiServer` transport binding over that contract.
- ADR-028 is now active in the current tree: the facade helpers are stored under `src/app/`, transport code has been extracted from `src/local_api.rs` into `src/server/` submodules (`http.rs`, `sse.rs`, `socket.rs`, `handlers/mod.rs`, `handlers/session.rs`, `util.rs`), and dependency-direction enforcement tests verify inward-only import rules across all layers, including grouped, multiline, and `super::`-relative `crate::{server::...}` / `crate::{bin::...}` imports.
- ADR-029 is now accepted: the stream parser covers all documented SSE event types (error envelopes, heartbeats, thinking/signature deltas, citations, server-tool blocks, web-search results, cache/geo/detail metadata) and TaskState persists plan, session notes, context compaction records, and cache usage stats for multi-agent handoff. ADR-029 is a declared dependency of ADR-030 and a prerequisite for full invariant compliance — `StreamEvent::Error` lets orchestrating agents detect sub-agent stream failures, and the TaskState extensions are the handoff payload that lets an orchestrator reconstruct a sub-agent's context on resume.
- ADR-030 is now accepted with an explicit six-point verification suite: provider events normalize into canonical runtime events, task state owns execution truth, the orchestrator decides whether the task continues or stops, and task handoff or resume consumers depend on that same runtime-owned control flow. ADR-030 is also load-bearing for multi-agent orchestration: Invariants 1, 4, and 5 are the semantic correctness guarantees that make agent handoffs coherent. Without these invariants proven end-to-end, multi-agent orchestration has undefined behaviour at handoff points.
- ADR-031 extends the active operator surface with timeline selection, stable step identity, explicit approved/running/completed lifecycle rendering, prompt-anchored transcript scrolling, a larger multiline composer, direct ANSI task rendering during orchestration, and keyboard navigation for timeline selection and inspector detail. Each pending tool call carries a stable `step_id` and compact input preview. The task-state timeline still derives pending rows as `AwaitingApproval`, `Approved`, or `Running` from canonical state, and the `Approved` state is tracked for manual approvals, session auto-approvals, and capability-grant auto-approvals. Batches A through E are merged into `main`. Batch C/D implemented viewport alignment (output-pane scroll ownership and six-line inspector cap) across both the direct ANSI and ratatui renderers. The fullscreen composer now also auto-fits to current display row and column changes, including narrower half-screen or quarter-screen display snaps. Batch E removed the legacy `activity_rows` derivation, `draw_timeline_fallback()`, `draw_legacy_activity_row()`, and the `legacy_row` field from `TaskStepView`, and the current ANSI path renders those task-state updates as transcript paragraphs instead of reserving a dedicated top strip.

- ADR-032 adds prompt-area interactivity: interactive `/` slash command picker and `@path` file picker with `Up`/`Down`/`Enter`/`Esc` navigation and hierarchical directory drill-down, `!command` shell execution, pasted-block handling, a responsive auto-fit composer surface that keeps those controls visible under display resize, and a context guard that limits project-instructions and notes token budgets.
- ADR-033 introduces the hybrid retrieval context architecture: a `codebase_search` tool (Phase 1) backed by structural keyword indexing, optional semantic vector search via an external embedding endpoint (Phase 2), write guards that steer `write_file` toward `apply_patch`/`edit_file` for large files (Phase 3), and history condensing that compresses older tool results to stay within the context budget (Phase 4).
- ADR-034 defines the proposed post-milestone multi-agent lane: worktree-isolated agent definitions, orchestrator-owned session-task lifecycle, `/agents`, `/watch`, and explicit session-task release surfaces, plus delegation-time concurrency and prompt-size enforcement built on the canonical ADR-025/ADR-030 contracts. The current hardening pass makes the delegation cap serialized, adds release-route and concurrency-stress coverage, and normalizes parent-task watch rollups onto the same lowercase status surface used by session tasks.
- ADR-038 is now Accepted for memory-first TTFC work. Phase 1 is merged in-tree: context assembly reuses a bounded process-local cache for small file rollups, and automatic git status/diff capture is opt-in rather than mandatory. Phase 1a added search lane tightening (search config during index warmup, incremental refresh independence from auto_index). Phase 2 adds `src/disk_policy.rs` (DiskPermission enum, check_path classifier, VEX_DISK_POLICY env) and `src/config/cache.rs` (OnceLock-based Config::load_cached). Batch C extracted `src/config/load.rs` (1361 lines) into a directory module: `src/config/load/paths.rs` (path discovery), `src/config/load/merge.rs` (layer merge helpers), and `src/config/load/parse.rs` (enum + header parsing), with orchestration and tests retained in `src/config/load/mod.rs`. Batch D splits `src/tools/operator.rs` (865 lines) into `src/tools/operator/mod.rs`, `core.rs`, `file_ops.rs`, `git_ops.rs`, and `search.rs`, preserving behavior while isolating the later disk-policy enforcement seam. Batch E on PR #281 splits `src/runtime/context_assembler.rs` into `src/runtime/context_assembler/mod.rs` (orchestration + tests) and `src/runtime/context_assembler/reads.rs` (candidate-path extraction, rollup conversion, related-path inference). Batch F on the same PR adds `enforce()` / `enforce_runtime()` to `src/disk_policy.rs`, `tests/disk_policy_tests.rs`, `make check-disk-policy`, and the `arch-contracts.yml` CI step. Batch G (PR #282) adds `src/tools/operator/policy.rs` for operator-boundary disk-policy assertions, wires `assert_durable_access()` into `TaskState::save()` and `TaskState::load()`, and fixes cross-platform `check_path()` for Windows backslash separators. Batch H (PR #283) extracts `src/runtime/task_state.rs` (807 lines) into `src/runtime/task_state/{mod.rs, persist.rs}`, isolating all persistence logic (save/load, directory discovery, file listing, active summary reads) into a dedicated module. WAL evaluation concluded: not warranted because task-state saves are per-session and `write_json_safe` already performs crash-safe writes (temp + fsync + rename). ADR-038 is now Accepted with 0 remaining items.

The transport layer (`src/server/`) now reaches the runtime exclusively through the application facade (`src/app/`), and `src/local_api.rs` retains only the `LocalApiMode` / `LocalApiFrontend` runtime-mode bridge types.
