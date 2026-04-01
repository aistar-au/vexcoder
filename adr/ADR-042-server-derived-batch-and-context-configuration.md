# ADR-042: Server-Derived Batch and Context Configuration

- **Status:** Accepted
- **Date:** 2026-04-01
- **Deciders:** Core maintainer
- **Depends on:** ADR-029, ADR-040
- **Supersedes:** None
- **Superseded by:** None

## Context

A local inference server session log (`server-log.txt`) shows the model generating
at ~1.5 tokens/second with `n_predict: 1024`, exhausting the generation
budget repeatedly on large file reads without completing useful work. The
model retries the same `read_file` call with different offset/limit
parameters, each retry consuming the full 1024-token budget. The root
cause is that `max_tokens` is resolved from a fixed default (1024) or
clamped environment variable (`VEX_MAX_TOKENS`) with no awareness of the
server's actual resource limits.

Additionally, the server exposes a 65536-token context window
(`n_ctx: 65536`) but the client never queries server capabilities. The
model's system prompt, tool definitions, and conversation history consume
a significant fraction of the context before generation begins, but the
client cannot calculate a safe `max_tokens` ceiling because the server's
`n_ctx` is unknown.

The session log also shows the model calling `run_shell_command` (a tool
that does not exist in the registered tool set), indicating the system
prompt or tool list is not sufficiently clear about available tools.

## Decision

### D1: Server-info polling for local endpoints

Before the first streaming request to a local endpoint (identified by
`is_local_endpoint_url()`), the client issues a lightweight GET to the
server's `/props` or `/v1/models` endpoint. The response provides:

- `n_ctx` (total context window)
- `n_batch` (decode batch size, affects throughput)
- `model` (loaded model identifier)

These values are stored in a `ServerInfo` struct and cached for the session
lifetime. The poll is best-effort: if the endpoint returns a non-2xx status
or an unrecognised schema, the client falls back to the existing defaults.

### D2: Derive max_tokens from server context

`resolve_max_tokens()` accepts the server's `n_ctx` as a plain `u32`. When
the server context is known (non-zero), `max_tokens` is capped at 75% of
`n_ctx` to leave headroom for the prompt:

```
ceiling = server_n_ctx × 0.75
max_tokens = clamp(user_override_or_default, 128, ceiling)
```

When `server_n_ctx` is zero (server unreachable or non-local), the ceiling
falls back to a generous constant of 16 384 tokens.

The `VEX_MAX_TOKENS` environment variable overrides the compiled-in model
default, but the result is still bounded by the server-derived ceiling.

### D3: Expose batch and context in telemetry

The turn-timing summary line emitted at the end of each turn already
reports prompt-eval and predict timing with token counts:

```
[ttft:1.2s | ↑:1.2s (512 tok) | ↓:52.0s (77 tok) | total:53.2s]
```

Exposing the server's `n_ctx` and effective `max_tokens` in that line is
tracked as a follow-up improvement once stable server-info polling is in
place.

### D4: Prevent stray tool calls

The `run_shell_command` tool name does not exist in the registered tool
set. The actual tool is `run_command`. To prevent models from hallucinating
tool names:

1. The system prompt now includes an explicit tool inventory sentence:
   "Available tools: read_file, write_file, edit_file, ... Do not call
   tools not in this list."
2. Unknown tool names in the structured tool-call path return an immediate
   error result (`Unknown tool: <name>`) instead of silently failing,
   matching the existing tagged-fallback behaviour.

## Consequences

- Local sessions automatically adapt `max_tokens` to the server's context
  window, preventing wasted generation budget on servers with large contexts.
- The operator can tune batch size and context via server flags
  (`--ctx-size`, `--batch-size`) and the client respects those settings
  without per-session env-var overrides.
- The telemetry line provides actionable capacity data for tuning.
- Models that hallucinate tool names receive a clear error instead of
  retrying indefinitely.
- Remote (non-local) endpoints are unaffected; the poll is skipped and
  existing defaults apply.

## Risks

- The `/props` endpoint is specific to one inference server family. Other local servers
  use different discovery endpoints. Mitigated: the poll is
  best-effort with graceful fallback, and can be extended with
  provider-specific adapters.
- Caching server info for the session assumes the server's context/batch
  config doesn't change mid-session. Mitigated: users can restart the
  session to pick up new settings, and the poll could be re-issued on
  context-overflow errors.
