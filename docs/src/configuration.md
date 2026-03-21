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
| `model_url` | Model endpoint URL | `http://localhost:11434/v1` |
| `model_url_skip_tls_check` | Skip HTTPS certificate validation for the model endpoint | `false` |
| `model_name` | Model identifier | `local/default` |
| `working_dir` | Workspace root for tool execution | current directory |
| `model_backend` | `local-runtime` or `api-server` | inferred |
| `model_protocol` | `messages-v1` or `chat-compat` | inferred |
| `tool_call_mode` | `structured` or `tagged-fallback` | inferred |
| `model_profile` | Path to a repo-tracked profile under `models/` | backend default profile |
| `max_project_instructions_tokens` | Project instructions token budget | `4096` |
| `max_memory_tokens` | Notes token budget | `2048` |
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

### `VEX_MODEL_TOKEN`

Bearer token for authenticated endpoints.

### `VEX_MODEL_URL_SKIP_TLS_CHECK`

Development-only escape hatch for HTTPS model endpoints with self-signed or
otherwise non-system-trusted certificates.

- Accepts `true`, `false`, `1`, or `0`.
- Emits a startup warning on every launch when enabled.
- Must not be committed in repo-local `.vex/config.toml`.

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

### `VEX_MAX_COMMAND_OUTPUT_BYTES`

Maximum bytes kept in the accumulated stdout/stderr buffer returned to the
model after a `run_command` tool call. The full output is always streamed to
the TUI transcript. Default: `51200` (50 KiB).

## `vex init` scaffold

`vex init` writes a commented config skeleton. It includes some reserved
sections for future expansion.

- The active runtime keys are the top-level keys listed above.
- `[[hooks]]` is active today.
- Commented `[api]` remains a scaffold placeholder in config files.
  `VEX_API_*` environment variables (transport, host, port, socket, key,
  protocol, TLS paths) are active and functional for API server configuration.
- `[[mcp_servers]]` and `sandbox_require` are not active runtime features yet,
  but `vex doctor` reads them to probe MCP connectivity and report sandbox
  fallback status.

## Minimal examples

Local endpoint:

```toml
model_url = "http://localhost:11434/v1"
model_name = "local/default"
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
