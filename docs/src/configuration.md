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
| `model_name` | Model identifier | `local/default` |
| `working_dir` | Workspace root for tool execution | current directory |
| `model_backend` | `local-runtime` or `api-server` | inferred |
| `model_protocol` | `messages-v1` or `chat-compat` | inferred |
| `tool_call_mode` | `structured` or `tagged-fallback` | inferred |
| `max_project_instructions_tokens` | Project instructions token budget | `4096` |
| `max_memory_tokens` | Notes token budget | `2048` |
| `notes_path` | Notes file used by `/memory` | unset |

`notes_path` is user-config only.

## Environment variables

### `VEX_MODEL_URL`

The full model endpoint URL.

- URLs containing `/chat/completions` or ending in `/v1` default to `chat-compat`.
- Other URLs default to `messages-v1`.

### `VEX_MODEL_TOKEN`

Bearer token for authenticated endpoints.

### `VEX_MODEL_NAME`

Model identifier sent to the API.

### `VEX_MODEL_PROTOCOL`

Overrides protocol inference. Accepted values: `messages-v1`, `chat-compat`.

### `VEX_MODEL_BACKEND`

Overrides backend inference. Accepted values: `local-runtime`, `api-server`.

### `VEX_TOOL_CALL_MODE`

Overrides tool-call encoding. Accepted values: `structured`,
`tagged-fallback`.

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

## `vex init` scaffold

`vex init` writes a commented config skeleton. It includes some reserved
sections for future expansion.

- The active runtime keys are the top-level keys listed above.
- `[[hooks]]` is active today.
- Commented `[api]` and `[[mcp_servers]]` blocks are scaffold placeholders.

## Minimal examples

Local endpoint:

```toml
model_url = "http://localhost:11434/v1"
model_name = "local/default"
```

Remote endpoint:

```toml
model_url = "https://api.example.internal/v1/messages"
model_name = "repo-assistant"
```

Token for authenticated endpoints:

```bash
export VEX_MODEL_TOKEN="your-token"
```
