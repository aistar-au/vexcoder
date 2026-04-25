# Privacy

VexCoder keeps operator state local by default, sends prompts and repository
context only to explicitly configured external services, and treats the
LocalApiServer transport as a separate privacy boundary.

This page covers the interactive CLI and the LocalApiServer surface implemented
in this tree. It does not replace the policies of the configured model
endpoint, MCP servers, or any other external service.

## Why this page is structured this way

Comparable local-first coding tools and local model servers commonly separate
four questions instead of flattening them into one legal block:

- what stays on the machine,
- what crosses a configured network boundary,
- how secrets are stored and transmitted, and
- what telemetry or retention rules apply after that boundary is crossed.

VexCoder adopts the same separation because the repository already implements
those seams in config loading, credential storage, task-state persistence,
transport binding, and LocalApiServer auth rules.

## CLI surface

The CLI surface stores operator-selected state on disk and sends task content
only to configured model or integration endpoints.

### What stays on this machine

- Saved task state, projections, and peer-message files are written under
  `.vex/state/` or `VEX_STATE_DIR`.
- Search indexes are written under `.vex/index/` when structural or semantic
  indexing is enabled.
- Notes are read from and written to the operator-selected `notes_path`, which
  is allowed only in the user config layer.
- Exported task artifacts are written only when `vex export` is invoked.
- Rolling file logs are written only when `RUST_LOG` is set.

### What may cross a configured boundary

- Prompts, selected repository context, tool results, and model outputs are
  sent to the configured model endpoint.
- Requests to configured MCP servers or HTTP hooks leave the machine only when
  those integrations are enabled.
- Provider-returned usage metadata may appear in transcript rows, saved task
  state, or exports because it arrives inside the model response.

### Protections and defaults

- Model credentials are read from `VEX_MODEL_TOKEN` or the OS credential store;
  `vex credentials set` refuses argv secrets.
- Repo-local config rejects `notes_path` and API secrets so repository state
  does not become the persistence layer for sensitive settings.
- The current build does not include a repository-managed analytics uploader;
  task telemetry shown in the surface is local runtime state.

## Local API surface

LocalApiServer exposes the shared runtime to local clients through loopback
HTTP or Unix sockets and applies a dedicated auth and transport boundary.

### What stays on the local surface

- Active pulse state, approval state, and streamed envelope buffers are held in
  memory while a task is running.
- Persistent task state still uses the same `.vex/state/` files as the CLI
  surface; the server does not create a second hosted history store.
- Unix-socket mode uses a filesystem path under `${XDG_RUNTIME_DIR}` or `/tmp`
  on supported platforms.

### What may cross a configured boundary

- Authorized LocalApiServer clients receive prompts, transcript rows, tool
  results, and pulse metadata over `POST /v1/pulses` and the related control
  routes.
- If the server is bound beyond loopback, those payloads travel over the
  configured TLS surface.
- Metadata endpoints such as `/v1/health`, `/v1/schema`, and `/v1/privacy`
  expose service or policy data rather than runtime envelopes.

### Protections and defaults

- HTTP mode requires a bearer token, and repo-local config may not provide
  `api.key`.
- Non-loopback HTTP requires TLS 1.2 or newer, with TLS 1.3 preferred, and
  HTTPS responses emit HSTS.
- Streaming responses set no-cache headers and disable proxy buffering on the
  LocalApiServer surface.

## Credentials

Credential handling is explicit and keeps long-lived secrets out of repo-local
config and argv surfaces.

- Model credentials are stored in the OS keyring or supplied through
  environment variables.
- LocalApiServer bearer tokens are user-config or environment only; repo-local
  config rejects them.
- `VEX_KEYRING_DISABLED` disables credential-store fallback when
  environment-only token handling is required.

## Telemetry and logs

Runtime telemetry is part of the local operator surface; external telemetry
paths are opt-in or provider-defined rather than repository-managed.

- Interactive pulse telemetry is displayed in the CLI transcript and status
  surface as local state rather than as a separate analytics export.
- When the configured model endpoint returns usage metadata, VexCoder may
  persist or render that metadata with the pulse because it is part of the model
  response.
- Crash or debug logs are opt-in via `RUST_LOG`; without it, the runtime does
  not create rolling log files.

## Retention

Retention is primarily operator-controlled for local files and service-controlled
for any configured external endpoint.

- Saved task state, peer-message files, indexes, and exports remain on disk
  until the operator removes them or changes the configured storage path.
- Notes remain at the configured `notes_path` until they are edited or deleted.
- Active LocalApiServer task buffers are retained in memory for the life of the
  active pulse and then released.
- Remote retention for model endpoints, MCP servers, and other external
  services is controlled by those services rather than by VexCoder.

## Operator controls

- Run against a same-machine local model endpoint when prompts and repository
  context must remain on the machine.
- Prefer Unix-socket mode or loopback HTTP for LocalApiServer clients.
- Keep `api.host` on loopback unless TLS and the bearer-token boundary are
  intentionally configured.
- Use `vex credentials` instead of literal secrets in shell history or
  repo-local config.
- Review this page before enabling remote hooks, MCP servers, or non-loopback
  LocalApiServer binds.