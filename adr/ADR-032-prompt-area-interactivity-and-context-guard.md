# ADR-032: Prompt Area Interactivity and Context Guard

- **Status:** Active
- **Date:** 2026-03-22
- **Deciders:** Core maintainer
- **Depends on:** ADR-031, ADR-015
- **Supersedes:** None
- **Superseded by:** None

## Context

Local inference servers (llama.cpp, vLLM, Ollama) expose a fixed context
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

### Token-aware offset reading

8. Tool-result file reads use offset-based reading (10s or 100s of lines)
   rather than full-file reads to preserve context budget on small-window
   servers.

## Consequences

- Users see the actual server error on context overflow instead of a
  misleading protocol hint.
- `/clear` becomes the documented recovery path for context exhaustion.
- Prompt area focus and character count reduce guesswork during input.
- Offset-based file reads reduce context waste for tool results.

## References

- [ADR-031](https://github.com/aistar-au/vexcoder/blob/main/adr/ADR-031-operator-surface-ui-overhaul.md) — operator surface UI overhaul
- [ADR-015](https://github.com/aistar-au/vexcoder/blob/main/adr/ADR-015-local-endpoint-text-protocol-default.md) — local endpoint text protocol default
