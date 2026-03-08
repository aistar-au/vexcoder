# Architecture Overview

This page describes VexCoder's runtime structure at a level sufficient to orient contributors. Detailed decisions and rationale are in the [Architecture Decision Records](adr/index.md).

## Runtime boundary

The runtime boundary separates orchestration from presentation. Everything inside `src/runtime/` owns orchestration and policy wiring. UI code in `src/ui/` and the application surface in `src/app.rs` do not reach through this boundary.

The canonical dispatch path from user input to tool execution is:

```
user input
  → app command handling (src/app.rs)
    → runtime loop (src/runtime/loop.rs)
      → edit loop (src/runtime/edit_loop.rs)
        → API client (src/api/client.rs)
          → tool operator (src/tools/operator.rs)
            → approval gate (src/runtime/approval.rs)
              → filesystem / shell tool execution
```

There is one path. There are no alternate routing shortcuts.

## Protocol detection

VexCoder supports two wire protocols. The protocol is detected automatically from the endpoint URL:

- `messages-v1` — native protocol, used when the path does not match `chat-compat` patterns.
- `chat-compat` — OpenAI-compatible chat completions protocol, used when the path contains `/chat/completions` or ends in `/v1`.

The backend mode (`api-server` vs `local-runtime`) is inferred by the same mechanism and can be overridden via `VEX_MODEL_BACKEND`.

## Edit loop

The edit loop (`src/runtime/edit_loop.rs`) is the bounded execution context for a single agentic turn. It enforces a turn ceiling (`DEFAULT_MAX_TURNS = 6`, `HARD_MAX_TURNS = 12`), supports cancellation via a `CancellationToken`, and tracks workspace dirty state. The last validation result is accessible after the loop completes.

## Tool confinement

All file operations from the tool operator (`src/tools/operator.rs`) are confined to the workspace root. Paths are normalised lexically before any filesystem access to prevent traversal outside the root.

## Headless and TUI modes

VexCoder supports both a terminal UI session and a headless mode for scripting. The mode boundary is enforced at the runtime seam. The same core runtime runs in both modes.

## State

Conversation and task state (`src/state/`) are managed separately. Conversation history, streaming state, and tool call tracking live in `src/state/conversation/`. Task state is tracked in `src/runtime/task_state.rs`.

## Further reading

- [ADR-004 — Runtime seam and headless-first design](../adr/completed/ADR-004-runtime-seam-headless-first.md)
- [ADR-006 — Runtime mode contracts](../adr/completed/ADR-006-runtime-mode-contracts.md)
- [ADR-023 — Deterministic edit loop](../adr/ADR-023-deterministic-edit-loop.md)
- [Full ADR index](adr/index.md)
