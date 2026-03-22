# ADR-032: Prompt Area Interactivity and Context Guard

- **Status:** Active
- **Date:** 2026-03-22
- **Deciders:** Core maintainer
- **Depends on:** ADR-031, ADR-015
- **Supersedes:** None
- **Superseded by:** None

## Context

Local inference servers expose a fixed context
window via `--ctx-size` or equivalent. When conversation history grows beyond
this limit the server returns HTTP 400 with a body describing the overflow.
The original error handler did not read the response body, so the user saw a
generic protocol-hint message instead of the actionable context-overflow
diagnosis.

The prompt area also lacked interactive feedback: no character count, no
focus indicator, and no in-session context recovery path. Users running
small-context local models had no way to know they were approaching the limit
or to recover without restarting.

## Decision

### Context-overflow error recovery

1. The API client reads the response body on any 4xx before constructing the
   error. Pattern matching on the body text distinguishes context-overflow
   errors from protocol mismatches.

2. Context-overflow errors surface the server's message verbatim (truncated
   to 300 characters) and append actionable guidance:
   - For local endpoints: suggest `--ctx-size <N>` and `/clear`.
   - For remote endpoints: suggest `/clear`.

3. Non-context-overflow 400s on local endpoints retain the existing protocol
   detection hint (MessagesV1 vs ChatCompat).

### Prompt area interactivity contracts

4. **Character count indicator** — the prompt status line shows a live
   character count so users can gauge input size relative to the context
   budget before submitting.

5. **Focus indicator** — the prompt border or status area visually
   distinguishes focused (active input) from unfocused (scrolling transcript)
   state.

6. **`/clear` for context recovery** — the existing `/clear` command resets
   conversation history. Context-overflow error messages now explicitly
   suggest `/clear` as the recovery path.

7. **`@` file picker** — the `@` prefix surfaces files from the current
   working directory with arrow-key navigation and Enter to select.

### Context-proportional offset reading

8. The `read_file` tool accepts `offset` (1-based line) and `limit`
   parameters. When no explicit limit is given, an auto-cap derived from
   `VEX_MAX_TOKENS` prevents full-file reads from exhausting the context
   window. The heuristic allocates ~10% of the context budget per file read
   at ~20 tokens per line:

   - 4K context: ~50 lines per read
   - 32K context: ~160 lines
   - 128K context: ~640 lines
   - 1M+ context: up to 10,000 lines

   Configurable via `VEX_READ_FILE_MAX_LINES` for explicit override.

### Target architecture: hybrid retrieval

9. The offset/limit mechanism is a pragmatic first step. The target
   architecture for large-codebase context management uses a hybrid
   retrieval pipeline:

   - **AST-aware chunking**: structural graph at function/type boundaries
     (Tree-sitter for Rust). Never reads a full file unless explicitly
     requested; returns snippets with file:line references.
   - **Semantic search tool** (`codebase_search`): vector-indexed semantic
     queries return ranked snippets, not whole files. Indexing is
     incremental and persistent.
   - **Diff-native edits**: `apply_diff` / patch-style edits preferred over
     full-file writes to prevent truncation on files exceeding ~500 lines.
   - **Task decomposition**: complex refactors decomposed into isolated
     subtasks with slim per-task context. Results summarized before passing
     back to the orchestrator.
   - **Context condensing**: conversation history auto-summarized; oldest
     messages dropped to stay under the model window.
   - **Observe-revise self-correction**: agent runs tests/linters on
     changes, observes failures, then uses index to pull just the broken
     part for the next turn rather than re-scanning files.

## Consequences

- Users see the actual server error on context overflow instead of a
  misleading protocol hint.
- `/clear` becomes the documented recovery path for context exhaustion.
- Prompt area focus and character count reduce guesswork during input.
- Context-proportional auto-cap prevents file reads from exhausting small
  context windows while allowing generous reads on large contexts.
- The hybrid retrieval target ensures the architecture scales to massive
  codebases without naive full-file reads.

## References

- [ADR-031](https://github.com/aistar-au/vexcoder/blob/main/adr/ADR-031-operator-surface-ui-overhaul.md) — operator surface UI overhaul
- [ADR-015](https://github.com/aistar-au/vexcoder/blob/main/adr/ADR-015-local-endpoint-text-protocol-default.md) — local endpoint text protocol default
