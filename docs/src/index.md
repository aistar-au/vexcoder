# vexcoder

Terminal-first coding assistant with streaming responses, tool execution, and ratatui UI.

## Quick Start

```bash
cargo run
```

## Configuration

`vexcoder` is configured via environment variables. `VEX_MODEL_URL` is the only required variable.

| Variable | Required | Description |
|---|---|---|
| `VEX_MODEL_URL` | Yes | API endpoint URL |
| `VEX_MODEL_TOKEN` | Remote only | Bearer token for non-local endpoints |
| `VEX_MODEL_NAME` | No | Model identifier (default: `local/default`) |
| `VEX_MODEL_PROTOCOL` | No | `messages-v1` or `chat-compat` (inferred from URL if omitted) |
| `VEX_TOOL_CALL_MODE` | No | `structured` (remote default) or `tagged-fallback` (local default) |
| `VEX_MODEL_BACKEND` | No | `api-server` or `local-runtime` (inferred from URL if omitted) |
| `VEX_WORKDIR` | No | Working directory override (defaults to current directory) |

`VEX_MODEL_PROTOCOL` is inferred from the URL: endpoints containing `/chat/completions` or ending in `/v1` default to `chat-compat`; all others default to `messages-v1`.

Local endpoint example:

```bash
VEX_MODEL_URL=http://localhost:8000/v1/messages \
VEX_MODEL_NAME=local/default \
cargo run
```

Remote endpoint example:

```bash
VEX_MODEL_URL=https://your-inference-server/v1/messages \
VEX_MODEL_TOKEN=your-token \
VEX_MODEL_NAME=your-model-name \
cargo run
```

For operators migrating from a pre-ADR-022 deployment, see the [ADR-022 Migration Guide](migration.md).

## Built-in TUI Commands

- `/commands` or `/help`
- `/clear`
- `/history`
- `/repo`
- `/ps`
- `/quit`

## Documentation Structure

This site is built with mdBook and published through GitHub Pages.

- Policy: [Documentation Policy](policy.md)
- Migration: [ADR-022 Migration Guide](migration.md)

ADR files are not stored in `docs/`.

- Open dispatch ADRs: `TASKS/ADR-*.md`
- Completed ADR records: `TASKS/completed/ADR-*.md`
- ADR index: `TASKS/ADR-README.md`

## Source Links

The canonical tracked source maps are maintained in:

- `CONTRIBUTING.md` (app-level Rust source map)
- `TASKS/completed/REPO-RAW-URL-MAP.md` (all tracked repository files)
