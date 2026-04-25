# Configuration

VexCoder reads configuration from layered TOML files plus environment
variables. The normal starting point is:

```bash
./target/release/vex init
```

If you have already installed `vex` onto your `PATH`, the equivalent command is
`vex init`.

## Resolution order

Highest priority wins:

1. Environment variables
2. Repo-scoped `.vex/config.toml`
3. User config: `~/.config/vex/config.toml` or `~/.vex/config.toml`
4. System config: `/etc/vex/config.toml`
5. Built-in defaults

`VEX_MODEL_TOKEN` is still forbidden in config files. At runtime, token lookup
prefers `VEX_MODEL_TOKEN` from the environment and then falls back to the OS
credential store unless `VEX_KEYRING_DISABLED` is set.

Automatic context assembly now keeps small file rollups in a process-resident
memory cache. Search indexes under `.vex/index/` and task-state JSON under
`.vex/state/` remain the intended disk-backed layers.

## Active config keys

These keys are read by the current runtime from config files:

| Key | Purpose | Default |
| :--- | :--- | :--- |
| `model_url` | Full model endpoint URL used by the runtime | unset; must be configured unless `api_client.base_url` is set |
| `model_url_skip_tls_check` | Skip HTTPS certificate validation for the model endpoint | `false` |
| `model_name` | Model identifier | `local/default` |
| `working_dir` | Workspace root for tool execution | current directory |
| `model_backend` | `local-runtime` or `api-server` | inferred |
| `model_protocol` | `messages-v1` or `chat-compat`; used as the fallback when `model_url` or `api_client.base_url` does not already pin a concrete endpoint path and local discovery has not selected a native protocol yet. When a bare base URL exposes both protocols, discovery prefers `messages-v1`. | `messages-v1` |
| `tool_call_mode` | `structured` only | inferred; resolves to `structured` in the current loader |
| `tool_policy` | `full`, `plan`, or `chat` | `full` |
| `api_client.base_url` | Scheme-and-host base URL used for automatic protocol discovery | unset |
| `model_profile` | Path to a repo-tracked profile under `models/` | default profile for the selected API backend |
| `max_project_instructions_tokens` | Project instructions token budget | `4096` |
| `max_memory_tokens` | Notes token budget | `2048` |
| `sandbox` | Command sandbox driver: `passthrough`, `macos-exec`, `container`, or `bubblewrap` | `passthrough` |
| `sandbox_profile` | Sandbox profile path, container image name, or extra bubblewrap arguments | unset |
| `sandbox_require` | Abort startup instead of falling back to passthrough when the sandbox probe fails | `false` |
| `notes_path` | Notes file used by `/memory` | unset |

`notes_path` is user-config only.

When `model_profile` is set, the runtime loads the profile at startup and uses
its request parameters (`temperature`, `top_p`, `max_tokens`, stop sequences,
reasoning budget, and structured-tool settings). Relative paths are resolved
from the workspace repo root when one is available, otherwise from the current
working directory.

## Tool-call formats

`tool_call_mode` controls the accepted downstream tool-call contract. The
current runtime accepts only `structured`, and supported upstream tool-call
shapes are normalized at API ingress before the event reaches downstream
consumers.

| Mode | Meaning | Current parser boundary |
| :--- | :--- | :--- |
| `structured` | Native structured tool calls are the accepted runtime mode | JSON `tool_calls` arrays and content-block `tool_use` payloads are parsed and normalized at API ingress |

The runtime currently recognizes two upstream tool-call shapes at API
normalization:

1. JSON `tool_calls` arrays from chat-completion style APIs.
2. Content-block `tool_use` records from block-oriented APIs.

These shapes form the accepted `tool_call_mode = "structured"` contract.

For chat-completion style tool calls, ingress tolerates the two local-server
argument encodings seen in practice: incremental JSON text fragments inside
`function.arguments` and fully materialized JSON values. Both are normalized to
the same typed runtime tool-call updates before the event reaches the CLI or
ratatui surface, so downstream consumers never parse server SSE frames or
provider-specific tool-call JSON directly.

That normalization rule follows RFC 8259's interoperability guidance: array
order is semantically significant, while object member order is not. The
runtime therefore coalesces streamed tool calls by array index at ingress and
projects parsed JSON values downstream instead of exposing raw object-member
ordering or transport-fragment details.

For local and private-network endpoints, the client also protects the accepted
boundary against stalled `stream = true` requests. If the local or
private-network server accepts the request but does not emit an initial SSE
event promptly, the client retries once with `stream = false` and feeds the
JSON response through the same ingress normalizer. The CLI and ratatui
consumers therefore continue to observe the same typed runtime events even
when the endpoint varies between SSE and full-response delivery.

These paths are distinct from `regex-lite` processing. `regex-lite` is used
for git output parsing, secret rewriting, and rate-limit extraction; it is not
used for live tool-call parsing.

## Tool policy

`tool_policy` controls which tools are exposed to the model for a session.

| Policy | Tools exposed | CLI option |
| :--- | :--- | :--- |
| `full` | All registered tools including mutating and shell tools | (default) |
| `plan` | Read-only tools only: `read_file`, `list_files`, `list_directory`, `list_dir`, `glob_files`, `search_files`, `search`, `git_status`, `git_diff`, `git_log`, `git_show`, `search_content`, `find_files`, `codebase_search`, plus any configured MCP tools | `--view-intended-trajectory` or `--restrict-payload-tools` |
| `chat` | No tools — plain conversation mode | config-only (`tool_policy = "chat"`) |

When `plan` is active, mutating tools (`write_file`, `apply_patch`, `edit_file`,
`rename_file`, `git_add`, `git_commit`, `run_command`) are excluded from the
model schema and rejected at the work layer as a defense-in-depth guard.

## Feature config sections

### `[compaction]`

Controls proactive conversation compaction. When enabled, the runtime compacts
the conversation history when the estimated token count approaches the context
budget, keeping recent pulses verbatim and folding older context into a summary.

| Key | Purpose | Default |
| :--- | :--- | :--- |
| `enabled` | Enable proactive compaction | `false` |
| `threshold_percent` | Compact when token usage exceeds this percentage of the context window (10--99) | `80` |
| `keep_recent_turns` | Number of most-recent pulses kept verbatim after compaction (1--32) | `4` |
| `summary_max_tokens` | Maximum tokens for the compaction summary (64--4096) | `1024` |

```toml
[compaction]
enabled = true
threshold_percent = 75
keep_recent_turns = 6
```

### `[undo]`

Controls the in-memory checkpoint stack used by `/undo`.

| Key | Purpose | Default |
| :--- | :--- | :--- |
| `enabled` | Whether `/undo` is available | `true` |
| `max_checkpoints` | Maximum checkpoints kept per session | `20` |

```toml
[undo]
enabled = true
max_checkpoints = 30
```

### `[search]`

Controls structural index builds and `codebase_search` behavior.
When `enabled = false`, both `codebase_search` and `/reindex` are unavailable.

| Key | Purpose | Default |
| :--- | :--- | :--- |
| `enabled` | Enable codebase search indexing | `true` |
| `auto_index` | Warm the structural index at interactive and batch session start | `true` |
| `exclude` | Workspace-relative path prefixes to exclude from indexing | `["target/", "node_modules/", ".git/"]` |
| `max_file_size` | Skip files larger than this byte count | `1048576` (1 MiB) |

Incremental index updates triggered by file writes during a session always
apply `exclude` and `max_file_size` filters regardless of the `auto_index`
setting. `auto_index` only controls whether the index is pre-warmed at
session startup.

`exclude` entries are literal workspace-relative prefixes, not glob patterns.
Use trailing slashes for directory trees such as `target/` or `src/vendor/`.
Entries missing a trailing slash are automatically normalized at config load
time (e.g. `"src"` becomes `"src/"`).

```toml
[search]
enabled = true
auto_index = true
exclude = ["target/", "node_modules/", ".git/", "src/vendor/"]
max_file_size = 524288
```

### `[auto_memory]`

Controls automatic memory extraction from assistant pulses. When enabled, short
factual notes are extracted after each pulse and appended to the notes file with
timestamped `[auto]` tags.

| Key | Purpose | Default |
| :--- | :--- | :--- |
| `enabled` | Enable automatic extraction | `false` |
| `max_notes_per_turn` | Maximum notes extracted per pulse (1--10) | `3` |

```toml
[auto_memory]
enabled = true
max_notes_per_turn = 5
```

## Environment variables

### `VEX_MODEL_URL`

The full model endpoint URL.

- `messages-v1` is always the default wire protocol regardless of URL path.
  If the local server only exposes `/v1/chat/completions`, `vex` detects this
  automatically at session start via the server discovery probe and switches to
  `chat-compat` without any manual configuration.
- For plain local inference servers, prefer explicit HTTP
  localhost URLs such as `http://localhost:8000/v1/messages`. If you enter an
  HTTPS localhost URL in the interactive startup prompt, `vex` now suggests the
  equivalent plain-HTTP localhost endpoint before the fullscreen session starts.
- Local and private-network inference runtimes commonly expose only plain HTTP. That
  remains supported when you connect via `localhost`,
  `127.x.x.x`, `::1`, or `0.0.0.0`. LAN-reachable model servers on
  RFC 1918 private addresses (`192.168.x.x`, `10.x.x.x`, `172.16–31.x.x`)
  and link-local addresses (`169.254.x.x`) are also allowed over plain HTTP.
  Only truly remote (public-internet) endpoints require HTTPS.
- If a local endpoint returns HTTP 400 due to context overflow, the error now
  shows the server's message verbatim and suggests increasing `--ctx-size` on
  the server or using `/compact` to reset the conversation.
- For non-context-overflow 400s, the error includes the inferred protocol
  (MessagesV1 vs ChatCompat) and suggests checking the model name, protocol
  format, and whether the server supports streaming.
- If a local or private-network endpoint accepts `stream = true` but does not
  emit an initial SSE event promptly, the client retries once with
  `stream = false` and normalizes the JSON response through the same API
  boundary instead of exposing a second downstream parser.

### API tracing and startup retries

For CLI troubleshooting, especially around `vex -p`, the transport layer now
emits request-scoped tracing, propagates W3C trace context, and applies a
short exponential backoff when a local endpoint is still starting up.

- `vex --display-internal-telemetry -p "..."` mirrors internal telemetry to
  stderr for one-shot runs without having to set `RUST_LOG` manually.
- `vex --telemetry-json --display-internal-telemetry -p "..."` emits
  newline-delimited JSON telemetry suitable for local collection by Vector,
  Fluent Bit, or an OpenTelemetry collector.
- `RUST_LOG=debug` enables structured transport, normalization, rate-limit,
  and fallback spans for both interactive and one-shot runs.
- `VEX_INTERNAL_TELEMETRY_PATH=/tmp/vex-internal.log` writes internal
  telemetry to a dedicated non-blocking file sink instead of stderr.
- `VEX_TELEMETRY_FORMAT=json` selects NDJSON output when telemetry is written
  to a file-backed sink.
- `VEX_TELEMETRY_ENVIRONMENT=staging` and `VEX_TELEMETRY_TENANT_ID=internal`
  attach environment and tenant baggage to the current trace context.
- `VEX_API_LOG_PATH=/tmp/vex-api.log` chooses where payload diagnostics are
  written when payload logging is enabled.
- `VEX_DEBUG_PAYLOAD=1` records outbound request payload summaries for API
  debugging. Prompt text, tool arguments, and SSE payload bodies are reduced to
  structural metadata rather than being written verbatim.
- Request and server spans carry `traceparent`, `x-request-id`, and GenAI
  semantic attributes so transport logs, tool lifecycle events, and runtime
  normalization can be correlated under one request trace.
- Local endpoint startup retries only cover initial connection failures before
  a response stream exists. Once a streaming request is established, the CLI
  does not replay it automatically.

### `VEX_MODEL_TOKEN`

Bearer token for authenticated endpoints.

Lookup order for authenticated endpoints:

1. `VEX_MODEL_TOKEN` when it is set and non-empty
2. OS credential store entry `service = "vexcoder"`, `account = "model-token"`

The credential-store fallback is disabled when `VEX_KEYRING_DISABLED=1`.
The current build uses platform-native credential stores.

Store the token without placing it on argv:

```bash
printf '%s' "$VEX_MODEL_TOKEN" | vex credentials set model-token --stdin
```

### `VEX_KEYRING_DISABLED`

Disable OS credential-store reads and writes.

- Accepts any non-empty value.
- When set, the runtime ignores the credential-store fallback and only honors
  `VEX_MODEL_TOKEN`.
- Useful for CI or debugging a credential-store issue.

### `VEX_MODEL_URL_SKIP_TLS_CHECK`

Development-only escape hatch for HTTPS model endpoints with self-signed or
otherwise non-system-trusted certificates.

- Accepts `true`, `false`, `1`, or `0`.
- Emits a startup warning on every start when enabled.
- Must not be committed in repo-local `.vex/config.toml`.

For any model endpoint outside local and private networks, HTTPS is mandatory.
Plain `http://` model URLs are rejected at startup for public-internet hosts so
prompts, repository context, and model responses are not sent over unencrypted
network paths. This rule does not block local inference servers reached via
`localhost`, `127.x.x.x`, `::1`, `0.0.0.0`, or RFC 1918 / link-local LAN
addresses (`192.168.x.x`, `10.x.x.x`, `172.16–31.x.x`, `169.254.x.x`).
`VEX_MODEL_URL_SKIP_TLS_CHECK` only relaxes certificate verification for HTTPS
endpoints; it does not permit plain HTTP for public-internet hosts.

The transport stack prefers TLS 1.3 and keeps TLS 1.2 enabled only for older
local and private inference servers that have not moved forward yet.

### `VEX_MODEL_NAME`

Model identifier sent to the API.

### `VEX_MODEL_PROTOCOL`

Overrides protocol inference. Accepted values: `messages-v1`, `chat-compat`.

The runtime still probes local endpoints and records the server-native protocol
for transport diagnostics. Use this override only when a server needs an
explicit wire-protocol pin instead of the discovered default.

### `VEX_MODEL_BACKEND`

Overrides API-backend inference. Accepted values: `local-runtime`, `api-server`.

### `VEX_TOOL_CALL_MODE`

Overrides tool-call encoding. Accepted value: `structured`.


### `VEX_MODEL_PROFILE`

Selects a repo-tracked model profile such as `models/api-structured.toml`.
An invalid or missing path is a startup failure.

### `VEX_WORKDIR`

Overrides the working directory used for tool execution.

### `VEX_MODEL_HEADERS_JSON`

Adds extra request headers as a JSON object.

Operator-supplied headers are preserved when internal tracing is enabled. The
runtime injects `x-request-id` and W3C trace context headers only when they are
not already present.

Example:

```bash
export VEX_MODEL_HEADERS_JSON='{"X-Client-Id":"vexcoder"}'
```

### `VEX_MAX_PROJECT_INSTRUCTIONS_TOKENS`

Overrides the project instructions token budget.

### `VEX_MAX_MEMORY_TOKENS`

Overrides the notes token budget.

### `VEX_CONTEXT_INCLUDE_GIT`

Opt in to automatic git status and diff injection during context assembly.

- Accepts `true`, `false`, `1`, `0`, `yes`, `no`, `on`, or `off`.
- Default: `false`.
- Explicit git tools and review flows still call git directly; this option only
  controls the automatic context path used before a normal model pulse.

### `VEX_CONTEXT_GIT_TIMEOUT_MS`

Controls the timeout used by context-related git commands.

- Default: `2000`.
- Applies to automatic git context when `VEX_CONTEXT_INCLUDE_GIT=1` and to the
  existing review helpers that call git through the shared runtime wrapper.

## API tracing and startup retries

- `--display-internal-telemetry` mirrors internal transport logs to stderr.
- `--telemetry-json` switches those logs to NDJSON/JSONL output.
- `VEX_INTERNAL_TELEMETRY_PATH` writes internal telemetry to a file instead of
  stderr.
- `VEX_TELEMETRY_FORMAT=json` is the environment equivalent of
  `--telemetry-json`.
- `VEX_TELEMETRY_ENVIRONMENT` and `VEX_TELEMETRY_TENANT_ID` add baggage fields
  to request-scoped spans.

When telemetry is enabled, the CLI installs a local OpenTelemetry tracer
provider, propagates W3C `traceparent` and `baggage` headers on outbound model
requests, and records structural payload summaries instead of raw prompts or tool
arguments.

Local API rate limiting applies only after bearer-token authorization succeeds.
Authorized requests are bucketed per forwarded client IP when available and
otherwise by an authorization-header-derived key, so unauthenticated callers do
not consume the same limit bucket as valid local clients.

### `VEX_DISK_POLICY`

Controls the disk-policy enforcement mode (ADR-038).

- Accepted values: `off`, `warn`, `strict`.
- Default: `off`.
- When set to `strict`, forbidden disk access (anything outside `.vex/index/`
  and `.vex/state/`) causes a panic. `warn` logs a warning instead.
- Intended for CI gates; not typically set in interactive use.

### `VEX_SANDBOX`

Selects the command sandbox driver. Accepted values: `passthrough`,
`macos-exec`, `container`, `bubblewrap`.

- `passthrough` preserves the current process-spawn behavior.
- `macos-exec` wraps commands with `sandbox-exec` on macOS.
- `container` wraps commands with the installed container runtime and requires
  `VEX_SANDBOX_PROFILE` to name the container image.
- `bubblewrap` wraps commands with `bwrap`, mounts the workspace read-write,
  mounts core system directories read-only, adds read-only mounts for common
  host toolchain roots derived from `PATH`, `CARGO_HOME`, and `RUSTUP_HOME`,
  and disables network by default.
- The built-in `macos-exec` profile keeps broad access by default: it
  allows broad file access, network access, process spawning, IPC lookups, and
  signals so common development tools continue to work. Use a custom profile if
  you need stricter containment than process wrapping plus policy hooks.

### `VEX_SANDBOX_PROFILE`

Optional sandbox driver parameter.

- For `macos-exec`, this is a profile path. When unset, the runtime uses a
  built-in permissive policy string.
- For `container`, this is the image name passed to the container runtime.
  Startup runs a short `run --rm <image> true` probe through that runtime so
  the selected image is validated before the first wrapped command.
- For `bubblewrap`, this is an optional whitespace-separated list of extra
  `bwrap` arguments appended before the `--` command separator. Quoting and
  escaping are not preserved.

### `VEX_SANDBOX_REQUIRE`

Controls startup fallback when the selected sandbox probe fails.

- Accepts `true`, `false`, `1`, or `0`.
- When `false`, startup emits a warning and falls back to `passthrough`.
- When `true`, startup aborts instead of running without containment.

### `VEX_MAX_TOKENS`

Upper bound override for the per-pulse generation budget. When set, the value
is treated as the maximum `max_tokens` for a single pulse. The runtime also
polls the local inference server's context size at startup and derives an
effective ceiling of 75% of `n_ctx`; the actual `max_tokens` sent is
`min(VEX_MAX_TOKENS, n_ctx × 0.75)`. When not set, the model profile's
`max_tokens` value serves as the default, still bounded by the server cap.
The runtime also derives per-file read limits and search result budgets from
the effective token budget when explicit overrides are not set.

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

Strict line limit for `write_file`. Calls exceeding this are rejected outright
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

Number of recent conversation pulses kept at full fidelity. Older pulses are
condensed: tool results keep their first 5 lines plus a
`(N more lines)` indicator, keeping the conversation within the context
budget without losing the thread of earlier work. Default: `10`.

### `VEX_MCP_TIMEOUT`

MCP server connection timeout in seconds applied to every configured server
at session start.  Each server entry may also set `timeout_secs` in the
config file; the per-server value takes priority over this environment
variable.  Range: 1–300.  Default: `30`.

## `vex init` generated files

`vex init` writes a commented config skeleton. It includes some reserved
sections for future expansion.

- The active runtime keys are the top-level keys listed above.
- `[[hooks]]` is active today.
- `sandbox`, `sandbox_profile`, and `sandbox_require` are active runtime
  features and apply to TUI, batch mode, inline `!command`, hooks, and
  validation subprocesses.
- `[[mcp_servers]]` is active today. MCP servers are connected at session start,
  loaded from the user config layer, and merged into the runtime tool registry
  as `mcp.<server>.<tool>` names. Servers are explicitly shut down when the
  session ends (TUI exit, batch completion, or API server stop).
- Commented `[api]` remains a placeholder section in config files.
  `VEX_API_*` environment variables (transport, host, port, socket, key,
  protocol, TLS paths) are active and functional for API server configuration.
- `[[mcp_servers]]` is rejected in repo-local and system config layers to avoid
  committed or machine-global auto-start of arbitrary MCP processes.

## MCP servers

Use `[[mcp_servers]]` only in the user config file. Each server is connected at
session start; load failures abort startup instead of leaving a partial MCP
registry in memory. Connected servers are explicitly cancelled at session end
via `McpRegistry::shutdown()`.

HTTP headers may be written literally, as bare `${NAME}` references, or as
templates that mix literal text with `${NAME}` segments resolved from the
current process environment.

```toml
[[mcp_servers]]
name = "docs"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]

[[mcp_servers]]
name = "remote"
transport = "http"
url = "https://mcp.example.internal/mcp"
timeout_secs = 60

[mcp_servers.headers]
Authorization = "Bearer ${VEX_MCP_AUTH}"
```

When MCP servers are loaded successfully:

- `/mcp list` shows the current server inventory.
- `/mcp show <server>` shows the tool names exported by one server.
- `/tools` includes both built-in tools and MCP tools.

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

## API Client Configuration (ADR-047)

The `[api_client]` section is live. `base_url` enables the ADR-047 host-and-port
connection path, `probe_timeout_ms` controls the discovery-request ceiling, and
`delta_accumulator_memory_watermark_mb` bounds in-progress tool-call delta
state. `explicit_protocol` remains available when an operator must pin request
routing instead of using discovery, but the normal path is automatic discovery
for both fullscreen and `vex -p` runs. Discovery probes `/v1/messages` first
and only falls back to `/v1/chat/completions` when the messages-v1 probe fails,
so dual-protocol servers stay on messages-v1 by default. When `model_url`
already points at `/v1/messages` or
`/v1/chat/completions`, that concrete endpoint path is preserved; discovery
rewrites only bare base URLs.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `base_url` | string | `""` | Optional scheme-and-host base URL (for example `http://127.0.0.1:8000`). When set, the client discovers or applies the protocol and adapts requests to `/v1/messages` or `/v1/chat/completions`. |
| `explicit_protocol` | `"messages-v1"` \| `"chat-compat"` | unset | Optional explicit protocol pin. When set, discovery is skipped and request routing stays on the selected protocol. |
| `probe_timeout_ms` | integer | `2000` | Per-request timeout budget for each ADR-047 protocol-discovery probe (`/v1/messages`, then `/v1/chat/completions`). Raise this for slower local runtimes or remote tunnels. |
| `delta_accumulator_memory_watermark_mb` | integer | `256` | Memory ceiling (MiB) for in-progress streaming tool-argument deltas. When exceeded, the oldest pending entry is evicted. |

Discovery-based form:

```toml
[api_client]
base_url = "http://127.0.0.1:8000"
probe_timeout_ms = 2000
delta_accumulator_memory_watermark_mb = 512
```

Pinned-protocol form:

```toml
[api_client]
base_url = "http://127.0.0.1:8000"
explicit_protocol = "chat-compat"
```
