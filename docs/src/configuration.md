# Configuration

VexCoder reads configuration from layered TOML files plus environment
variables. The normal starting point is:

```bash
vex init
```

## Resolution order

Highest priority wins:

1. Environment variables
2. Repo-local `.vex/config.toml`
3. User config: `~/.config/vex/config.toml` or `~/.vex/config.toml`
4. System config: `/etc/vex/config.toml`
5. Built-in defaults

`VEX_MODEL_TOKEN` is environment-only. It is never read from config files.

## Active config keys

These keys are read by the current runtime from config files:

| Key | Purpose | Default |
| :--- | :--- | :--- |
| `model_url` | Model endpoint URL | `http://localhost:8080/v1` |
| `model_url_skip_tls_check` | Skip HTTPS certificate validation for the model endpoint | `false` |
| `model_name` | Model identifier | `local/default` |
| `working_dir` | Workspace root for tool execution | current directory |
| `model_backend` | `local-runtime` or `api-server` | inferred |
| `model_protocol` | `messages-v1` or `chat-compat` | inferred |
| `tool_call_mode` | `structured` or `tagged-fallback` | inferred |
| `model_profile` | Path to a repo-tracked profile under `models/` | backend default profile |
| `max_project_instructions_tokens` | Project instructions token budget | `4096` |
| `max_memory_tokens` | Notes token budget | `2048` |
| `sandbox` | Command sandbox driver: `passthrough`, `macos-exec`, or `docker` | `passthrough` |
| `sandbox_profile` | Sandbox profile path or Docker image name | unset |
| `sandbox_require` | Abort startup instead of falling back to passthrough when the sandbox probe fails | `false` |
| `notes_path` | Notes file used by `/memory` | unset |

`notes_path` is user-config only.

When `model_profile` is set, the runtime loads the profile at startup and uses
its request parameters (`temperature`, `top_p`, `max_tokens`, stop sequences,
reasoning budget, and structured-tool fallback). Relative paths are resolved
from the workspace repo root when one is available, otherwise from the current
working directory.

## Environment variables

### `VEX_MODEL_URL`

The full model endpoint URL.

- URLs containing `/chat/completions` or ending in `/v1` default to `chat-compat`.
- Other URLs default to `messages-v1`.
- For plain local inference servers, prefer explicit HTTP
  localhost URLs such as `http://localhost:8000/v1/messages`. If you enter an
  HTTPS localhost URL in the interactive startup prompt, `vex` now suggests the
  equivalent plain-HTTP localhost endpoint before the fullscreen session starts.
- Same-machine local inference runtimes commonly expose only plain HTTP. That
  remains supported when you connect via `localhost`,
  `127.x.x.x`, `::1`, or `0.0.0.0`.
- If a local endpoint returns HTTP 400 due to context overflow, the error now
  shows the server's message verbatim and suggests increasing `--ctx-size` on
  the server or using `/compact` to reset the conversation.
- For non-context-overflow 400s, the error includes the detected protocol
  (MessagesV1 vs ChatCompat) and suggests checking the model name, protocol
  format, and whether the server supports streaming.

### `VEX_MODEL_TOKEN`

Bearer token for authenticated endpoints.

### `VEX_MODEL_URL_SKIP_TLS_CHECK`

Development-only escape hatch for HTTPS model endpoints with self-signed or
otherwise non-system-trusted certificates.

- Accepts `true`, `false`, `1`, or `0`.
- Emits a startup warning on every launch when enabled.
- Must not be committed in repo-local `.vex/config.toml`.

For any model endpoint that does not resolve to `localhost`, `127.x.x.x`,
`::1`, or `0.0.0.0`, HTTPS is mandatory. Plain `http://` remote model URLs are
rejected at startup so prompts, repository context, and model responses are not
sent over unencrypted network paths. This rule does not block same-machine
local inference servers when they are reached over one of the local addresses
above. `VEX_MODEL_URL_SKIP_TLS_CHECK` only relaxes certificate
verification for HTTPS endpoints; it does not permit plain HTTP for
non-loopback hosts.

### `VEX_MODEL_NAME`

Model identifier sent to the API.

### `VEX_MODEL_PROTOCOL`

Overrides protocol inference. Accepted values: `messages-v1`, `chat-compat`.

### `VEX_MODEL_BACKEND`

Overrides backend inference. Accepted values: `local-runtime`, `api-server`.

### `VEX_TOOL_CALL_MODE`

Overrides tool-call encoding. Accepted values: `structured`,
`tagged-fallback`.

### `VEX_MODEL_PROFILE`

Selects a repo-tracked model profile such as `models/api-structured.toml`.
An invalid or missing path is a startup failure.

### `VEX_WORKDIR`

Overrides the working directory used for tool execution.

### `VEX_MODEL_HEADERS_JSON`

Adds extra request headers as a JSON object.

Example:

```bash
export VEX_MODEL_HEADERS_JSON='{"X-Client-Id":"vexcoder"}'
```

### `VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS`

Overrides the project instructions token budget.

### `VEX_MAX_MEMORY_TOKENS`

Overrides the notes token budget.

### `VEX_SANDBOX`

Selects the command sandbox driver. Accepted values: `passthrough`,
`macos-exec`, `docker`.

- `passthrough` preserves the current process-spawn behavior.
- `macos-exec` wraps commands with `sandbox-exec` on macOS.
- `docker` wraps commands with `docker run` and requires
  `VEX_SANDBOX_PROFILE` to name the container image.

### `VEX_SANDBOX_PROFILE`

Optional sandbox driver parameter.

- For `macos-exec`, this is a profile path. When unset, the runtime uses a
  built-in default policy string.
- For `docker`, this is the image name passed to `docker run`.

### `VEX_SANDBOX_REQUIRE`

Controls startup fallback when the selected sandbox probe fails.

- Accepts `true`, `false`, `1`, or `0`.
- When `false`, startup emits a warning and falls back to `passthrough`.
- When `true`, startup aborts instead of running without containment.

### `VEX_MAX_TOKENS`

Base context-window size used by auto-cap calculations. The runtime derives
per-file read limits and search result budgets from this value when explicit
overrides are not set. Inferred from the model profile when available.

### `VEX_MAX_COMMAND_OUTPUT_BYTES`

Maximum bytes kept in the accumulated stdout/stderr buffer returned to the
model after a `run_command` tool call. The full output is always streamed to
the TUI transcript. Default: `51200` (50 KiB).

### `VEX_READ_FILE_MAX_LINES`

Maximum lines returned by the `read_file` tool when no explicit `limit`
parameter is provided. When not set, derives from `VEX_MAX_TOKENS`: roughly
10% of the context budget at ~20 tokens per line.

| Context budget | Auto-cap |
| :--- | :--- |
| 4 K tokens | ~50 lines |
| 32 K tokens | ~160 lines |
| 128 K tokens | ~640 lines |
| 1 M+ tokens | up to 10,000 lines |

The `read_file` tool also accepts `offset` (1-based line number) and `limit`
parameters for targeted partial reads.

### `VEX_DIFF_PREFERRED_ABOVE_LINES`

Line threshold above which `write_file` emits a warning suggesting
`apply_patch` or `edit_file` instead. The model sees the warning in the tool
result and is expected to switch strategy on the next attempt. Default: `200`.

### `VEX_WRITE_FILE_MAX_LINES`

Hard line limit for `write_file`. Calls exceeding this are rejected outright
with an error directing the model to use `apply_patch` or `edit_file`.
Default: `500`.

### `VEX_SEARCH_MAX_RESULTS`

Maximum number of results returned by the `codebase_search` tool. Default:
`10`.

### `VEX_INDEX_MAX_FILES`

Maximum number of files indexed for semantic search. Default: `5000`.

### `VEX_EMBEDDING_PROVIDER`

Embedding provider for semantic search. Accepted values: `compat` (standard
`/v1/embeddings` compatible endpoint) or `native` (single-text embedding
endpoint). Semantic search is disabled when this variable is unset.

### `VEX_EMBEDDING_MODEL`

Model identifier sent to the embedding endpoint. Required when
`VEX_EMBEDDING_PROVIDER` is set.

### `VEX_EMBEDDING_URL`

Base URL for the embedding endpoint. Required when `VEX_EMBEDDING_PROVIDER`
is set.

### `VEX_EMBEDDING_API_KEY`

Bearer token for authenticated embedding endpoints. Set this explicitly for
the embedding endpoint when required; the runtime does not fall back to
`VEX_MODEL_TOKEN`.

### `VEX_EMBEDDING_BATCH_SIZE`

Number of texts sent per embedding API call. Default: `32`.

### `VEX_HISTORY_KEEP_TURNS`

Number of recent conversation turns kept at full fidelity. Older turns are
condensed: tool results are truncated to their first 5 lines plus a
`(N more lines)` indicator, keeping the conversation within the context
budget without losing the thread of earlier work. Default: `10`.

## `vex init` scaffold

`vex init` writes a commented config skeleton. It includes some reserved
sections for future expansion.

- The active runtime keys are the top-level keys listed above.
- `[[hooks]]` is active today.
- `sandbox`, `sandbox_profile`, and `sandbox_require` are active runtime
  features and apply to TUI, batch mode, inline `!command`, hooks, and
  validation subprocesses.
- Commented `[api]` remains a scaffold placeholder in config files.
  `VEX_API_*` environment variables (transport, host, port, socket, key,
  protocol, TLS paths) are active and functional for API server configuration.
- `[[mcp_servers]]` is still a reserved section on this branch. `vex doctor`
  reads it to probe configured MCP connectivity without enabling live runtime
  MCP dispatch.

## Minimal examples

Local endpoint:

```toml
model_url = "http://localhost:8080/v1"
model_name = "local/default"
model_profile = "models/local-balanced.toml"
```

Local Messages-v1 endpoint example:

```toml
model_url = "http://localhost:8000/v1/messages"
model_name = "your-model-name"
model_profile = "models/local-balanced.toml"
```

Remote endpoint:

```toml
model_url = "https://api.example.internal/v1/messages"
model_name = "repo-assistant"
model_profile = "models/api-structured.toml"
```

Token for authenticated endpoints:

```bash
export VEX_MODEL_TOKEN="your-token"
```
