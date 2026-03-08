# Configuration

VexCoder is configured entirely through environment variables. There are no configuration files. The only required variable is `VEX_MODEL_URL`.

## Required

### `VEX_MODEL_URL`

The full URL of the inference API endpoint. VexCoder infers the protocol from this value.

- A path containing `/chat/completions` or ending in `/v1` defaults to `chat-compat` (OpenAI-compatible).
- All other paths default to `messages-v1`.

Local example:

```bash
VEX_MODEL_URL=http://localhost:8000/v1/messages
```

Remote example:

```bash
VEX_MODEL_URL=https://your-inference-server/v1/messages
```

## Authentication

### `VEX_MODEL_TOKEN`

Bearer token sent in the `Authorization` header. Required for remote endpoints that enforce authentication. Not needed for unauthenticated local endpoints.

## Model selection

### `VEX_MODEL_NAME`

The model identifier passed to the API. Defaults to `local/default` when not set. For remote endpoints this typically needs to match the model name the server recognises.

## Protocol and transport

### `VEX_MODEL_PROTOCOL`

Overrides the protocol inferred from the URL. Accepted values: `messages-v1`, `chat-compat`. Useful when your endpoint path does not follow the inference convention.

### `VEX_MODEL_BACKEND`

Overrides the backend mode inferred from the URL. Accepted values: `api-server`, `local-runtime`. VexCoder infers this from the URL; set it only if the inference is wrong for your setup.

### `VEX_TOOL_CALL_MODE`

Controls how tool calls are encoded in requests. Accepted values: `structured` (default for remote endpoints), `tagged-fallback` (default for local endpoints). The `tagged-fallback` mode embeds tool use in the message text for models that do not support structured tool call APIs.

## Working directory

### `VEX_WORKDIR`

Overrides the working directory used for tool execution. Defaults to the current directory at launch. VexCoder confines all file access to this root.

## Migration note

If you are migrating from a deployment predating the current protocol architecture, see [ADR-022](../adr/ADR-022-free-open-coding-agent-roadmap.md) and its [amendment](../adr/ADR-022-amendment-2026-03-03.md) for the changes to endpoint inference and protocol selection.
