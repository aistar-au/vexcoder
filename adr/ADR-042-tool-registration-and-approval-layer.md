# ADR-042: Tool Registration and Defense-in-Depth Approval Layer

- **Status:** Accepted (amended 2026-04-07 — original: Server-Derived Batch and Context Configuration)
- **Date:** 2026-04-01
- **Amended:** 2026-04-07
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

The session log also shows the model calling `run_shell_command` (a name
that does not exist in the registered tool set), indicating a mismatch
between the names the model expects from training data and the names
actually registered in the schema.

### Tool registration landscape (amendment context)

Research across agent CLI tools confirms the following patterns for shell
tool registration:

- Frontier-model systems register a single canonical name in the schema.
  The model calls that exact name.
- Local/open-weight models are not fine-tuned to the same fidelity. They
  extrapolate from training data patterns where `run_shell_command`, `bash`,
  `execute_command`, and `execute_bash` all appeared as legitimate tool names
  in different codebases. Without schema anchoring, they pick among these names
  inconsistently.
- The approval gate pattern (per-call user confirmation, scoped grants, and
  sandbox wrapping) is universal across agent CLI tools that expose shell
  access to the model.

The original D5 decision ("keep `run_command` frontend-only") addressed the
hallucination problem by excluding shell tools from the schema entirely. This
amendment replaces that decision with a schema-registered, multi-name alias,
defense-in-depth approval approach that resolves the name-mismatch root cause
rather than working around it.

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

Unknown tool names in the structured tool-call path return an immediate
error result (`Unknown tool: <name>`) instead of silently failing,
matching the existing tagged-fallback behaviour. The system prompt includes
an explicit tool inventory sentence to reduce spurious tool calls.

### D5: Multi-name alias registration with schema entry (amendment)

**Supersedes the original D5 ("keep `run_command` frontend-only").**

`run_command` is registered in the model-facing tool schema. Additionally,
the following commonly-hallucinated names are registered as dispatch-level
aliases routing to the same executor:

| Name                | Registered in schema | Dispatch action          |
|---------------------|----------------------|--------------------------|
| `run_command`       | Yes (canonical)      | `execute_run_command_tool` |
| `run_shell_command` | No (dispatch alias)  | `execute_run_command_tool` |
| `bash`              | No (dispatch alias)  | `execute_run_command_tool` |
| `execute_command`   | No (dispatch alias)  | `execute_run_command_tool` |
| `execute_bash`      | No (dispatch alias)  | `execute_run_command_tool` |

The schema exposes only `run_command` (the canonical name). This keeps the
schema compact while ensuring that any of the commonly-hallucinated aliases
still routes through the approval gate rather than returning "Unknown tool".

**Rationale for this design:**

1. Schema registration of `run_command` gives fine-tuned and frontier models
   a schema anchor to call without hallucination. The model sees the exact
   registered name and calls it when shell access is needed.
2. Dispatch-level aliases for the remaining names mean that a local model
   hallucinating `run_shell_command` or `bash` is not silently rejected —
   it is routed through the same approval gate, giving the user visibility
   and control.
3. Only one name appears in the schema, preventing the failure mode where
   the model has multiple shell names and picks unpredictably among them.

### D6: Defense-in-depth approval overlay

Every `run_command` execution (including all dispatch aliases from D5)
passes through the approval overlay unconditionally:

1. **Schema declaration gate**: The tool description states that shell
   commands require user approval on each invocation.
2. **`tool_requires_confirmation()` gate**: The formatter checks this
   function before constructing the approval overlay prompt. All five
   alias names are included in the confirmation set.
3. **`request_tool_approval()` gate**: The runtime sends a
   `ToolApprovalRequest` to the UI and blocks until the user responds.
4. **`ApprovalScope` grant tracking**: After approval, the scope (`once`,
   `task`, or `session`) is recorded in the session's `active_grants` map.
   Repeated calls within the same scope do not re-prompt.
5. **`SandboxDriver` wrapping**: The `CommandRequest` is wrapped by the
   configured sandbox before execution. The default `PassthroughSandbox`
   imposes no additional isolation; operators can substitute a restricted
   sandbox via the `SandboxConfig` in `config.toml`.

**Enforcement points:**

- `tool_definitions()` in `src/api/client/tools.rs` registers `run_command`
  with a description that explicitly states approval is required.
- `tool_requires_confirmation()` in `src/state/conversation/tools/formatting.rs`
  lists `run_command`, `run_shell_command`, `bash`, `execute_command`, and
  `execute_bash`.
- `execute_tool_with_timeout_with_updates()` in
  `src/state/conversation/tools/mod.rs` normalizes alias names to
  `run_command` before dispatch so the approval flow is uniform.
- The system prompt states that `run_command` is registered and requires
  user approval.

## Consequences

- Local sessions automatically adapt `max_tokens` to the server's context
  window, preventing wasted generation budget on servers with large contexts.
- The operator can tune batch size and context via server flags
  (`--ctx-size`, `--batch-size`) and the client respects those settings
  without per-session env-var overrides.
- The telemetry line provides actionable capacity data for tuning.
- Models that call any of the registered alias names now receive an
  approval prompt rather than an error, stopping the retry loop caused
  by `Unknown tool: run_shell_command`.
- The user retains full control: every shell invocation requires explicit
  approval (unless the session or task scope has been granted).
- Remote (non-local) endpoints are unaffected by D1; the server-info poll
  is skipped and existing defaults apply.

## Risks

- Registering `run_command` in the schema increases the likelihood that
  local models attempt shell calls in contexts where they are not needed.
  Mitigated: the approval gate requires explicit user confirmation at the
  `once` scope by default; auto-approve requires an explicit operator flag
  (`--auto-approve task`).
- Dispatch aliases for unregistered names (`bash`, `run_shell_command`,
  etc.) broaden the execution surface beyond what the schema alone implies.
  Mitigated: every alias routes through `tool_requires_confirmation()` and
  the `request_tool_approval()` gate before any command runs.
- The `PassthroughSandbox` default provides no filesystem or network
  isolation. Shell commands run with the same permissions as the vexcoder
  process. Operators who need stronger isolation must configure a
  `SandboxConfig` with a restricted sandbox driver.
- Extending `tool_requires_confirmation()` to cover aliases adds five
  string literals; a future rename of an alias that is not reflected here
  would silently skip the confirmation gate. Mitigated: a test asserts that
  all five names require confirmation.
- The `/props` endpoint is specific to one inference server family. Other
  local servers use different discovery endpoints. Mitigated: the poll is
  best-effort with graceful fallback, and can be extended with
  provider-specific adapters.
- Caching server info for the session assumes the server's context/batch
  config does not change mid-session. Mitigated: users can restart the
  session to pick up new settings, and the poll could be re-issued on
  context-overflow errors.
