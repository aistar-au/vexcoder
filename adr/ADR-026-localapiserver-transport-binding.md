# ADR-026: LocalApiServer transport binding — scoped HTTP/Unix-socket API surface, SSE streaming, TLS boundary, auth model, and schema endpoint for runtime JSON handoff

**Date:** 2026-03-11
**Status:** Proposed
**Deciders:** Core maintainer
**Location:** `adr/ADR-026-localapiserver-transport-binding.md`
**ADR chain:** ADR-025 (runtime JSON handoff contract), ADR-024 (Phase I reservation), ADR-006 (runtime mode contracts)
**Related:** ADR-023 (deterministic edit loop), ADR-022 (roadmap)

---

## Context

ADR-024 reserves `LocalApiServer` as the post-milestone-1 path for exposing the shared runtime core to rich local clients. It requires a dedicated ADR specifying:

- the wire protocol,
- the local authentication model,
- the streaming response format.

ADR-025 now defines the missing transport-neutral machine-readable contract: `RuntimeRequest` and `RuntimeEnvelope`.

This ADR therefore does **not** need to invent a new event schema. Its job is narrower and more precise:

> bind ADR-025's canonical runtime JSON contract to a concrete local transport so the runtime can be driven through a local API without duplicating runtime logic.

For milestone 1, CLI/TUI remains the primary operator surface. `LocalApiServer` exists to project ADR-025 envelopes across HTTP or socket boundaries without duplicating runtime behavior, so later adapters can hand off tasks and events as canonical JSON. Browser-specific origin policy, headers, and any in-tree web UI remain deferred.

**Checklist continuation note:** ADR-024 Phase I checklist items PI-01 through PI-08 cover session lifecycle and command-surface work. **Note:** PI-08 (`/plan` and `/context`) is tracked in ADR-023 EL-11/EL-12 and is only listed in ADR-024 for cross-reference. ADR-025 continues the LocalApiServer track from PI-09 through PI-12 for the canonical JSON handoff contract. This ADR continues from PI-13 through PI-16 for transport binding. ADR-028 defines the application-facade and transport-boundary rule that later CLI and server work must respect. A reconciliation change must keep ADR-024's Phase I checklist and config-key section aligned with ADR-025 and ADR-026 before transport work is treated as merge-ready.

---

## Sequencing guard

This ADR satisfies ADR-024's Phase I specification requirement, but implementation must not begin until:

1. Phase H (macOS packaging and distribution) is complete, and
2. milestone-1 correctness work (ADR-022 phases 1–8 plus ADR-023 deterministic edit loop) is validated end-to-end.

No dispatcher may begin LocalApiServer implementation before that gate is green.

---

## Decision

Introduce `LocalApiServer` as a transport adapter over the canonical ADR-025 contract.

### 1. Architectural role

`LocalApiServer` is the third `RuntimeMode + FrontendAdapter` implementation after `TuiMode` and `BatchMode`.

It does not define new runtime semantics. It projects the existing runtime through a scoped API surface.

Normative rule:

- runtime logic remains in Rust core/runtime code;
- transport parsing, auth checks, and stream framing live in the server adapter;
- native clients, editor panels, local automation, and any later JSON-capable adapter all consume the same canonical event model over a scoped transport surface.

This ADR defines a JSON transport adapter, not a browser frontend contract. CORS, origin allowlists, browser auth flows, and in-tree web-UI behavior require a later ADR if that surface is added.

### 2. Canonical request and event payloads

All request bodies and all streamed server payloads use ADR-025's canonical JSON contract:

- inbound: `RuntimeRequest`
- outbound: `RuntimeEnvelope`

This ADR forbids a server-specific event schema.

### 3. Supported transports

Phase I supports exactly two transports:

1. **HTTP**
   - default bind address: `127.0.0.1`
   - default port: `6274`
   - non-loopback TCP exposure is permitted only when TLS is enabled
2. **Unix-domain socket**
   - default path: `${XDG_RUNTIME_DIR}/vexcoder.sock` when available
   - fallback path: `/tmp/vexcoder.sock`

Loopback remains the default bind mode. Non-loopback TCP exposure requires explicit operator configuration and the TLS rules below.

**Port rationale:** default port `6274` is chosen as a high, non-privileged tooling port. It avoids privileged-port requirements and remains overrideable through user config (`VEX_API_PORT` environment variable or `api.port` config key).

**Platform support:**

- Unix-domain socket transport is supported on macOS and Linux only.
- HTTP transport is supported on macOS, Linux, and Windows.
- Windows clients must use HTTP transport.
- On Windows, operators should rely on host firewall rules whenever non-loopback TCP exposure is enabled.

**Recommended internet exposure pattern — loopback plus tunnel proxy:** when operators want internet reachability in Phase I, the preferred topology is to keep `LocalApiServer` bound to loopback and place an outbound tunnel proxy on the same host in front of it. A local tunnel daemon that forwards internet requests to `127.0.0.1:6274` is the intended example, so from `LocalApiServer`'s perspective the connection remains loopback HTTP with bearer auth. In that topology, the tunnel provider handles the external TLS surface. Operators using a custom `api.port` must update the tunnel target accordingly. This guidance does not relax the rules for direct non-loopback TCP binds in §3.1.

### 3.1 TLS boundary (normative)

- **Same machine, same user, private IPC:** prefer Unix-domain socket transport. Loopback HTTP may omit TLS when bearer auth is present.
- **Loopback detection rule:** for TLS gating, a host is loopback only when it is an IPv4 address in `127.0.0.0/8`, the IPv6 loopback `::1`, or the hostname `localhost` resolving exclusively to those loopback addresses. Any other literal address, hostname, or mixed-resolution result is treated as non-loopback.
- **TCP beyond strict loopback:** TLS is the default and is required in Phase I. This includes LAN, VPN, container-bridge, SSH-tunneled shared-host, reverse-proxied, browser-accessible, and other meaningful network exposure.
- **Minimum TLS version:** TLS 1.2 is the minimum supported version for non-loopback TCP exposure. TLS 1.3 is preferred. TLS 1.0 and 1.1 are forbidden in Phase I.
- **Private-network certificate source:** for local, LAN, and VPN deployments, the certificate may be self-signed, issued by an internal/private CA, or issued by a public CA. This ADR requires encrypted transport on non-loopback TCP; it does not require public-WebPKI issuance for private-network use.
- **Trust distribution note:** clients that rely on self-signed or internal-CA certificates must install or reference the appropriate trust material. That trust-distribution mechanism is adapter-specific and remains outside this ADR.
- **Sensitive payload rule:** if prompts, session state, repo contents, tool results, or authentication tokens cross any non-local network path, TLS is mandatory.
- **Reserved VPN carve-out:** a future ADR may allow authenticated, encrypted overlay networks such as Tailscale or WireGuard to satisfy the transport-encryption requirement when binding directly to a VPN virtual IP. That carve-out is not defined here. Until such an ADR exists, VPN addresses follow the ordinary non-loopback TLS rule. `api.vpn_trust` is a reserved config key only and must remain `false` in Phase I.
- **`transport = "both"` split:** the HTTP surface follows the same TLS rules as `transport = "http"`. The Unix-socket surface is unaffected by TLS configuration and continues to use filesystem-based auth.
- **Informational outbound note:** ADR-024 applies the same sensitive-payload reasoning to outbound `model_url` connections. That outbound rule is not enforced by PI-15 in this ADR; it remains an ADR-024 / ADR-022 concern.
- **Internet-facing advisory:** public-internet exposure is a stronger threat model than ordinary LAN use. TLS plus bearer auth is necessary but not sufficient. Until a later ADR defines built-in rate limiting and stronger client-auth options, operators should treat loopback plus tunnel proxy or loopback plus a hardening reverse proxy as the recommended internet path rather than a direct public bind.
- **Multi-user or enterprise serving:** TLS plus certificate validation, certificate rotation, and service authentication are conventional hardening requirements. Those broader serving semantics remain out of scope for this ADR unless a later ADR widens them explicitly.

### 4. HTTP API surface

The initial API surface is minimal. On loopback it may be served as plain HTTP; on any non-loopback TCP bind it must be served with TLS.

When HTTPS is enabled, responses should emit a `Strict-Transport-Security` header. Plain loopback HTTP must not emit HSTS.

#### `GET /v1/health`

Returns:

```json
{"ok":true,"service":"vexcoder-local-api","version":1}
```

Purpose: local readiness and client startup probe.

**Schema note:** `/v1/health` is intentionally simple and is not itself validated against an ADR-025 schema document. The `version` field refers to the LocalApiServer protocol version (currently `1`), not the ADR-025 schema version. To determine ADR-025 schema version, clients use `GET /v1/schema`. Breaking changes to the health response require a LocalApiServer protocol version bump.

#### `GET /v1/schema`

Returns the versioned ADR-025 schema bundle.

Normative rule:

- `request_schema` must contain the complete schema document from ADR-025 Appendix C (`schemas/runtime_request_v1.json`) verbatim.
- `envelope_schema` must contain the complete schema document from ADR-025 Appendix B (`schemas/runtime_envelope_v1.json`) verbatim.

The JSON below is an abbreviated extract showing the bundle shape only; it is not the complete schema payload.

```json
{
  "version": 1,
  "request_schema": {
    "$id": "https://vexcoder.io/schemas/runtime_request_v1.json",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "title": "RuntimeRequest v1",
    "oneOf": [
      {
        "type": "object"
      }
    ]
  },
  "envelope_schema": {
    "$id": "https://vexcoder.io/schemas/runtime_envelope_v1.json",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "title": "RuntimeEnvelope v1",
    "type": "object",
    "required": ["version", "task_id", "turn", "seq", "event"]
  }
}
```

This is the canonical client codegen and validation endpoint for LocalApiServer.

**Schema endpoint validation note:** The `/v1/schema` response is pre-flight metadata, not a `RuntimeEnvelope`. It is explicitly exempt from the outbound envelope validation rule in §9. Clients must not attempt to validate the schema bundle response against `runtime_envelope_v1.json`.

#### `POST /v1/turns`

Request body:

```json
{
  "type": "submit_input",
  "task_id": null,
  "input": "review src/app.rs"
}
```

The request body must validate as ADR-025 `RuntimeRequest`.

Response:

- `Content-Type: text/event-stream`
- `Cache-Control: no-cache`
- stream of SSE events where each `data:` payload is one ADR-025 `RuntimeEnvelope`

This endpoint is the only turn-submission endpoint in Phase I.

#### `POST /v1/approve`

Request body:

```json
{
  "type": "approve_capability",
  "task_id": "task-1741700000000",
  "capability": "run_command",
  "scope": "once"
}
```

The request body must validate as ADR-025 `RuntimeRequest` (`approve_capability` or `deny_capability` variant).

Response:

```json
{"ok":true}
```

This endpoint allows external clients to respond to `ApprovalRequest` envelopes received on an active SSE stream. It is a control endpoint; it does not use SSE.

Error responses:

- `{"ok":false,"reason":"task_not_found"}` with HTTP `404` if the `task_id` does not correspond to an active turn.
- `{"ok":false,"reason":"no_pending_approval"}` with HTTP `409` if the `task_id` is valid but no pending `ApprovalRequest` exists for the specified capability.

#### `POST /v1/interrupt`

Request body:

```json
{
  "type": "interrupt",
  "task_id": "task-1741700000000"
}
```

Response:

- `{"ok":true}` if the `task_id` corresponds to an active in-flight turn.
- `{"ok":false,"reason":"task_not_found"}` with HTTP `404` if the `task_id` does not correspond to an active turn (including already-completed turns). Interrupt of a completed or unknown task is not silently treated as success.

Interrupt does not use SSE. It is a control endpoint.

### 5. SSE binding rules

The HTTP streaming format is Server-Sent Events.

Each SSE message uses a single event name:

```text
event: runtime
data: {"version":1,"task_id":"task-1741700000000","turn":1,"seq":1,"event":{"type":"turn_start","input":"review src/app.rs"}}
```

Normative SSE rules:

- event name is always `runtime`;
- the semantic event type is read from `RuntimeEnvelope.event.type`;
- one `RuntimeEnvelope` object is emitted per SSE `data:` payload;
- no custom SSE event-name taxonomy such as `chunk`, `done`, or `error` is introduced;
- turn completion is indicated by ADR-025 `turn_end`, not by a transport-specific event name.

**Keepalive rule:** the server must emit an SSE comment line (`: keepalive`) at least once every 15 seconds during an active turn with no outbound envelopes, to prevent proxy and browser timeout disconnects. SSE comment lines are valid per the SSE spec and are ignored by compliant clients.

**Migration note:** existing code such as `src/api/stream.rs` currently parses upstream/provider SSE event taxonomies including `chunk` / `done` on the client side. That parser is provider-facing and is not the LocalApiServer contract. When LocalApiServer lands, LocalApiServer clients must parse `event: runtime` and read semantic state from `RuntimeEnvelope.event.type`.

### 6. Unix-socket binding rules

For Unix-domain socket transport, the payload format is line-delimited JSON:

- one inbound `RuntimeRequest` per line
- one outbound `RuntimeEnvelope` per line

This is a transport framing rule only. The canonical contract remains ADR-025.

The Unix-socket transport is intended for local native clients that prefer a socket over HTTP.

The server must create the socket with mode `0600` and owned by the launching user. If it cannot create or retain those permissions, startup fails rather than falling back to a broader permission set.

**Socket lifecycle rules:**

- On startup: if a socket file already exists at the target path (stale socket from a crashed prior process), the server must remove it before binding.
- On clean shutdown: the server must remove the socket file.
- Client reconnect behavior is client-defined; the server has no notion of session continuity across socket reconnections.

### 7. Authentication model

#### Unix-domain socket

Authentication is filesystem-based:

- socket file permissions must be `0600`;
- the socket must be owned by the launching user;
- if permissions are broader than `0600`, server startup fails with a diagnostic.

No bearer token is used for Unix socket transport.

#### HTTP / HTTPS

HTTP authentication is mandatory.

The client must send:

```text
Authorization: Bearer <token>
```

Token sources permitted:

- `VEX_API_KEY` environment variable
- user config layer only (`api.key` in user config)

Token sources forbidden:

- repo-local config
- project instructions
- any committed file in the working tree

If HTTP transport is enabled and no token is available, server startup fails.

Loopback HTTP may omit TLS. Any non-loopback TCP bind (`0.0.0.0`, a LAN address, a VPN address, or a container-bridge address) must enable TLS in addition to bearer auth. Plain HTTP on non-loopback TCP is forbidden in Phase I.

This avoids the contradiction of an HTTP API that claims authenticated sessions while allowing anonymous or plaintext network callers.

### 8. Configuration keys

Add the following canonical config keys:

```toml
# ~/.config/vex/config.toml — user config layer only for secrets

[api]
transport = "http"          # "http" | "unix" | "both"
host      = "127.0.0.1"    # default loopback; non-loopback TCP requires TLS in Phase I
                            # examples of non-loopback binds (all require tls_cert + tls_key):
                            #   host = "0.0.0.0"          # all interfaces
                            #   host = "192.168.x.x"      # specific LAN interface
                            #   host = "100.x.y.z"        # VPN virtual IP; still requires TLS in Phase I
                            # tunnel-proxy operators leave host as 127.0.0.1
port      = 6274            # override via VEX_API_PORT
socket    = ""              # empty = platform default; Unix only
key       = "${VEX_API_KEY}"  # env-var reference; repo-local rejected
tls_cert  = ""              # PEM certificate path; may be self-signed, internal-CA-issued, or public-CA-issued
tls_key   = ""              # PEM private key path; required for non-loopback TCP
tls_ca_cert = ""            # operator reference: PEM CA bundle path distributed to clients when tls_cert chains to a self-signed or internal CA; validated when non-empty, but not consumed by the server's inbound TLS stack
tls_skip_verify = false     # RESERVED false only; certificate-verification bypass is rejected in Phase I
vpn_trust = false           # RESERVED false only; VPN carve-out requires a dedicated ADR; TLS still required in Phase I
```

**Transport mode auth rules:**

- `transport = "http"` requires bearer auth; server startup fails without a valid `key`.
- loopback detection for TLS gating uses `127.0.0.0/8`, `::1`, and `localhost` only when `localhost` resolves exclusively to those loopback addresses.
- loopback HTTP may run without TLS.
- non-loopback HTTP must enable TLS; startup fails unless both `tls_cert` and `tls_key` are readable, valid PEM, form a matching certificate/private-key pair, and negotiate TLS 1.2 or newer. Expired or not-yet-valid certificates emit a startup warning.
- `tls_ca_cert` may be provided when clients need a non-system trust bundle for a self-signed or internal-CA `LocalApiServer` certificate. The server validates the bundle when non-empty, but does not consume it for inbound TLS termination.
- `tls_skip_verify` must remain `false` in Phase I.
- `transport = "unix"` uses filesystem auth; `key` is ignored and `api.key` need not be configured.
- `transport = "both"` enables both surfaces simultaneously. HTTP surface requires bearer auth and follows the same TLS rules as `transport = "http"`; Unix surface uses filesystem auth and ignores TLS configuration. The presence of a valid `key` is still required at startup when the HTTP surface is active under `"both"`.

**Forbidden values:**

- repo-local `api.key` is rejected at config load time;
- non-loopback TCP host values without valid TLS configuration are rejected in Phase I;
- `tls_skip_verify = true` is rejected in Phase I;
- `vpn_trust = true` is rejected until a dedicated ADR defines the VPN carve-out.

**Certificate lifecycle note:** certificate hot-reload is deferred in Phase I. `tls_cert`, `tls_key`, and `tls_ca_cert` are loaded and validated at startup only; rotating them requires a server restart.

**ADR-024 reconciliation (pre-merge requirement):** the `api.*` config-key block above must be applied to ADR-024's `Config TOML canonical keys` section before this ADR is merged. ADR-024 is currently `Proposed` status, so this is an in-place amendment consistent with the Proposed-status editing convention. A separate amendment ADR is not required unless ADR-024 is locked before this reconciliation PR lands. The reconciliation PR must also extend ADR-024's Phase I dispatcher checklist from PI-09 through PI-16.

**Pre-merge checklist:**

- [x] Apply the `[api]` config-key block to ADR-024's `Config TOML canonical keys` section (in-place amendment; ADR-024 is Proposed).
- [x] Extend ADR-024's Phase I dispatcher checklist from PI-09 through PI-16. ADR-025 owns PI-09–12; this ADR owns PI-13–16. The reconciliation PR is owned by the ADR-025 dispatcher (PI-09 closeout) and must be merged before PI-13 begins.
- [x] Verify all JSON, GBNF, and schema examples in ADR-025 and ADR-026 remain syntax-clean after reconciliation.

### 9. Validation at the API boundary

Every inbound and outbound payload is validated against ADR-025 schemas.

Validation order:

```text
HTTP(S) / socket request
    ↓
parse JSON
    ↓
validate against runtime_request_v1.json
    ↓
RuntimeCore / RuntimeMode processing
    ↓
build RuntimeEnvelope
    ↓
validate against runtime_envelope_v1.json
    ↓
emit over SSE or socket
```

If outbound validation fails, the server must not emit invalid JSON. It must log the failure and terminate the affected request/stream.

**Exemption:** `GET /v1/health` and `GET /v1/schema` responses are not `RuntimeEnvelope` payloads and are explicitly exempt from envelope validation.

### 10. Error semantics

Transport errors and runtime errors are distinct.

- **Runtime errors** are represented inside the stream as ADR-025 `RuntimeEnvelope.event = error`.
- **Transport errors** are represented by HTTP status codes, socket disconnects, or request rejection before streaming begins.

Rules:

- malformed JSON request → HTTP `400` / socket request rejection
- schema-invalid request → HTTP `422`
- missing or invalid bearer token → HTTP `401` (covers both missing and malformed/invalid token; `403` is not used by this API)
- unsupported path/method → HTTP `404` / `405`
- internal server failure before stream start → HTTP `500`
- runtime failure after stream start → emit ADR-025 `error` envelope, then terminate with a final `turn_end` when possible

Example runtime error envelope:

```json
{"version":1,"task_id":"task-1741700000000","turn":1,"seq":4,"event":{"type":"error","code":"tool_execution_failed","message":"Permission denied: /etc/passwd","recoverable":true}}
```

### 11. Backward compatibility with BatchMode

`LocalApiServer` does not replace `BatchMode` and does not alter `vex exec --format jsonl`.

The relationship is:

- BatchMode: file/CLI-oriented summarized JSONL
- LocalApiServer: live streaming projection of ADR-025 envelopes

They are complementary, not competing surfaces.

### 12. Explicit exclusions

This ADR does **not** authorize:

- `vex remote-control` / remote environment serving (deferred indefinitely per ADR-024; requires a dedicated ADR separate from Phase I `LocalApiServer`)
- plaintext non-loopback TCP exposure
- `api.vpn_trust` enforcement or any VPN-specific relaxation of the non-loopback TLS rule
- OAuth flows
- WebSocket support
- multi-user or multi-tenant serving
- browser-specific origin policy, CORS behavior, or in-tree web-UI behavior
- cloud-hosted deployment
- server-defined alternate event schemas

Any of those requires a separate ADR.

---

## Example HTTP stream

### Request

```http
POST /v1/turns HTTP/1.1
Host: 127.0.0.1:6274
Authorization: Bearer dev-local-token
Content-Type: application/json

{"type":"submit_input","task_id":null,"input":"review src/app.rs"}
```

### Response

```text
HTTP/1.1 200 OK
Content-Type: text/event-stream
Cache-Control: no-cache

event: runtime
data: {"version":1,"task_id":"task-1741700000000","turn":1,"seq":1,"event":{"type":"turn_start","input":"review src/app.rs"}}

event: runtime
data: {"version":1,"task_id":"task-1741700000000","turn":1,"seq":2,"event":{"type":"assistant_delta","text":"Analyzing runtime flow..."}}

event: runtime
data: {"version":1,"task_id":"task-1741700000000","turn":1,"seq":3,"event":{"type":"tool_call","id":"call_1741700123456_9a2f","name":"read_file","arguments":{"path":"src/app.rs"}}}

event: runtime
data: {"version":1,"task_id":"task-1741700000000","turn":1,"seq":4,"event":{"type":"tool_result","tool_call_id":"call_1741700123456_9a2f","tool_name":"read_file","is_error":false,"output":"..."}}

event: runtime
data: {"version":1,"task_id":"task-1741700000000","turn":1,"seq":5,"event":{"type":"assistant_message","content":"Analyzing runtime flow..."}}

event: runtime
data: {"version":1,"task_id":"task-1741700000000","turn":1,"seq":6,"event":{"type":"turn_end","status":"completed","usage":{"input":184,"output":67,"estimated":false},"changed_files":[]}}
```

---

## Integration with ADR-024 checklist

| ADR-024 item | How ADR-026 satisfies it |
|--------------|--------------------------|
| ADR-024 Phase I reservation | Binds ADR-025's canonical JSON contract to concrete local transports |
| ADR-024 Gap 2 (BatchMode) | Preserves existing `vex exec --format jsonl` and keeps BatchMode as a separate summarized surface |
| ADR-024 Gap 5 (MCP) | Carries canonical `mcp.<server>.<tool>` tool names unchanged over HTTP and Unix-socket transports |
| ADR-024 Gap 28 (Token counter) | Transports optional `TurnEnd.usage` without making PL-03 a prerequisite for transport work |
| Phase I wire protocol | Defines HTTP and Unix-socket bindings over ADR-025 JSON |
| Streaming response format | Defines SSE binding for HTTP and line-delimited JSON for Unix socket |
| Authentication model | Defines mandatory HTTP bearer auth, TLS on non-loopback TCP, and Unix-socket filesystem auth |
| Default bind posture | Loopback remains the default; non-loopback TCP is gated by TLS config validation |

---

## Rationale

### Why use a single SSE event name?

Once ADR-025 defines the semantic event type inside the JSON payload, duplicating event semantics in SSE event names is unnecessary and error-prone.

A single event name keeps the transport thin:

- SSE carries framed messages
- JSON carries meaning

This reduces client complexity and avoids contradictions such as "fatal error" being expressible both as an SSE event class and as a JSON event type.

### Why make HTTP bearer auth mandatory?

A CLI-owned HTTP API is still an API. Loopback is helpful but not sufficient as the only guard because other local processes may connect.

Mandatory bearer auth for HTTP keeps the rule simple and auditable:

- HTTP always authenticates
- Unix socket always uses filesystem permissions

It also matches ADR-024's explicit concern about repo-local secret sourcing and supply-chain protection.

### Why require TLS on non-loopback TCP?

The architectural boundary is not "has an API"; it is "has meaningful network exposure."

For same-machine, same-user private IPC, Unix sockets or loopback HTTP are sufficient. Once the transport crosses strict loopback, bearer auth alone no longer protects confidentiality or endpoint identity. At that point, TLS is the conventional default.

This ADR therefore treats LAN, VPN, container-bridge, reverse-proxied, browser-accessible, and other non-local TCP paths as TLS-required in Phase I whenever they carry prompts, session state, repo contents, tool results, or tokens.

The loopback plus tunnel-proxy pattern above is not an exception to this rule. It is a different topology: `LocalApiServer` itself remains loopback-bound, and the external TLS surface lives in the tunnel or proxy layer.

### Why keep Unix socket and HTTP both?

They solve different local-client needs:

- HTTP + SSE is convenient for browser views, editor panels, and general local tooling
- Unix socket is convenient for native local clients that want direct process-to-process communication without HTTP framing

Both transports carry the same canonical ADR-025 payloads, so supporting both does not fragment the event model.

### Why not add WebSocket now?

SSE already solves the required streaming path for Phase I with less protocol surface and simpler client/server behavior. Bidirectional control is already covered by ordinary POST endpoints.

WebSocket would widen the scope without adding unique architectural value at this stage.

### Why does `POST /v1/interrupt` return `404` for unknown task ids?

Silently returning `{"ok": true}` for an unknown task id would prevent clients from detecting the case where an interrupt was sent to a task that had already completed or never existed. A `404` with `{"ok": false, "reason": "task_not_found"}` makes the distinction explicit and allows clients to implement reliable interrupt-then-check flows.

### Why is ADR-024 reconciliation an in-place amendment rather than a new amendment ADR?

ADR-024 is `Proposed`, not `Locked`. The project convention (established by ADR-022's amendment) is to use a separate amendment ADR only when the parent ADR is `Locked` or `Accepted` and immutable. A `Proposed` ADR can be amended in-place by the same reconciliation PR that closes out the work. The pre-merge checklist in §8 records this explicitly so the amendment is traceable without a separate ADR document.

---

## Alternatives considered

### Bind LocalApiServer directly to BatchMode JSONL

Rejected. BatchMode JSONL is summarized output, not a canonical live event stream.

### Invent a server-specific `StreamChunk` schema separate from ADR-025

Rejected. That would recreate the duplication this ADR pair is intended to remove.

### Allow anonymous HTTP access on loopback

Rejected. This weakens the security boundary and creates ambiguity about what "authenticated local API" means.

### Use custom SSE event names (`chunk`, `done`, `error`)

Rejected. JSON payloads already carry typed events. A second taxonomy in SSE metadata invites drift.

### Return `{"ok": true}` for interrupt on unknown task id (idempotent)

Rejected. Idempotent no-op would prevent clients from detecting that their interrupt target was already gone. Explicit `404` is more useful and does not add implementation complexity.

---

## Consequences

**Easier after this ADR:**

- the runtime can be driven as a local JSON API without duplicating runtime behavior;
- clients can codegen against `/v1/schema`;
- browser and editor clients can consume live runtime events with a standard SSE client;
- socket-native clients can consume the same contract without HTTP;
- approval decisions can be driven from external clients via `POST /v1/approve`.

**Harder or more complex:**

- every emitted envelope must validate before transmission;
- auth/config handling now becomes part of Phase I correctness;
- transports must map runtime failures and transport failures distinctly;
- SSE keepalive adds a recurring background task for the server.

**Constraints imposed on future work:**

- no LocalApiServer code may invent a schema that is not ADR-025 `RuntimeRequest` / `RuntimeEnvelope`;
- HTTP transport must require bearer auth in Phase I;
- any non-loopback TCP exposure must require TLS in Phase I;
- repo-local config may not provide `api.key`;
- default host remains loopback-only, but explicit non-loopback TCP binds are allowed when TLS is configured;
- direct public-internet exposure should be documented as an operator-hardening path, not as the default Phase I topology;
- `vex remote-control` remains prohibited under this ADR;
- `transport = "both"` must enforce HTTP bearer auth on the HTTP surface; Unix surface uses filesystem auth independently.

---

## Dispatcher checklist

| ID | Task | Status |
|----|------|--------|
| **PI-13** | Implement `LocalApiServer` transport adapter with `POST /v1/turns`, `POST /v1/interrupt`, `POST /v1/approve`, and `GET /v1/health` | [ ] |
| **PI-14** | Implement `GET /v1/schema` serving ADR-025 schema bundle; exempt from envelope validation | [ ] |
| **PI-15** | Add Unix-socket transport, HTTP bearer auth, TLS-required non-loopback TCP, explicit loopback detection (`127.0.0.0/8`, `::1`, `localhost` resolving only to loopback), minimum TLS version 1.2 with 1.3 preferred, private-network certificate support (self-signed/internal-CA/public-CA), `tls_cert`/`tls_key` PEM and key-match validation, `tls_ca_cert` operator trust-bundle support, explicit `tls_skip_verify` rejection, reserved `vpn_trust` config guard, stale-socket cleanup, clean-shutdown socket removal, `transport = "both"` HTTP-vs-Unix split, config guards, and repo-local secret rejection | [ ] |
| **PI-16** | Add integration tests for SSE stream order, SSE keepalive emission, auth failures (`401` for missing/invalid token), loopback classification (`127.0.0.1`, other `127/8`, `::1`, and `localhost`), non-loopback-without-TLS rejection, TLS 1.2 minimum enforcement, `tls_cert`/`tls_key` mismatch rejection, `tls_skip_verify=true` rejection, `vpn_trust=true` rejection until a dedicated ADR exists, schema validation, mid-stream runtime error, `MaxTurnsReached` sequence, `POST /v1/interrupt` with unknown task id returns `404`, `POST /v1/approve` with unknown task id returns `404` and with no pending approval returns `409`, and reconnect/new-turn behavior | [ ] |

---

## Dispatcher reporting contract

When checking any PI-13…PI-16 box, append an evidence block:

```markdown
### [PI-XX] - <short title>
- Dispatcher: <name/id>
- Commit: <sha>
- Files changed:
  - `path/to/file` (+X -Y)
- Validation:
  - `cargo test --all-targets` : pass
  - `bash scripts/check_no_alternate_routing.sh` : pass
  - `bash scripts/check_forbidden_imports.sh` : pass
- Notes:
  - <what was built and why>
```

---

## Compliance notes for agents

**Relationship to ADR-024 compliance notes:** the rules below are additive to ADR-024's compliance table. All ADR-024 compliance rules remain in effect. Where a rule here overlaps ADR-024, this ADR narrows it for the LocalApiServer / runtime-JSON seam rather than replacing it.

| This ADR rule | ADR-024 cross-reference |
|--------------|------------------------|
| repo-local API secrets remain forbidden | same supply-chain posture as `VEX_MODEL_TOKEN` and `[[mcp_servers]]` restrictions |
| MCP tools must use `mcp.<server>.<tool>` names | Gap 5 / PF-01 namespace contract |
| `vex remote-control` remains out of scope | Phase I exclusion / deferred-indefinitely boundary |
| local transport permissions and guards are additional containment rules | additive to ADR-024's sandbox/config safety posture |

| Rule | Enforcement |
|------|-------------|
| Do not invent a server-only event schema | Must use ADR-025 envelopes verbatim |
| Do not expose HTTP without bearer auth in Phase I | Mandatory startup/config guard |
| Do not read `api.key` from repo-local config | Reject at load time |
| `VEX_API_KEY` must not appear as a literal in committed files | Enforce via secret/config validation and CI repository checks |
| Loopback detection for TLS gating uses `127.0.0.0/8`, `::1`, and `localhost` resolved only to loopback addresses | Any other literal or resolved address is non-loopback |
| Do not bind non-loopback TCP without TLS in Phase I | Reject LAN, VPN, container-bridge, public, and other non-loopback addresses unless valid TLS is configured |
| Minimum TLS version is 1.2; TLS 1.3 is preferred | Reject TLS negotiation below 1.2 on the non-loopback HTTP surface |
| `tls_cert` and `tls_key` must be readable valid PEM and represent a matching pair | Reject startup on parse or match failure; expired or not-yet-valid certs emit a startup warning |
| `tls_ca_cert` is operator trust-distribution metadata, not server-side trust bypass | Validate readability when non-empty; the server does not consume it for inbound TLS termination |
| Do not implement `tls_skip_verify` in Phase I | Certificate-verification bypass defeats the confidentiality and endpoint-identity guarantees that motivate TLS |
| Do not implement `api.vpn_trust` without a dedicated ADR | A VPN-specific carve-out requires an explicit rule set; until then VPN addresses follow the ordinary non-loopback TLS requirement |
| Tunnel-proxy internet exposure keeps `LocalApiServer` in loopback mode | Tunnel proxies target `127.0.0.1:6274` by default; if `api.port` changes, operator docs must update the tunnel target |
| Direct internet exposure must not be presented as the default Phase I path | Operator docs must warn that public binds need upstream rate limiting and hardening beyond TLS plus bearer auth |
| `transport = "both"` applies TLS rules only to the HTTP surface | Unix-socket transport remains filesystem-auth only and is unaffected by TLS config |
| Do not use SSE event-name taxonomy as semantic state | Event name is always `runtime`; semantics live in JSON |
| LocalApiServer clients must parse `event: runtime` only | Ignore semantic transport taxonomies such as `chunk`/`done` for this server |
| Do not alter `vex exec --format jsonl` here | BatchMode remains unchanged |
| Do not implement WebSocket here | Explicitly out of scope |
| Do not implement `vex remote-control` / remote environment serving here | Deferred indefinitely per ADR-024; requires dedicated ADR separate from Phase I |
| Every outbound payload (except `/v1/health` and `/v1/schema`) must validate against ADR-025 schema before emission | Mandatory API-boundary validation |
| Unix-socket permissions must be `0600` | Server must create the socket with `0600` and fail startup if broader or unfixable |
| Stale socket file must be removed at startup before binding | Prevents `EADDRINUSE` on restart after crash |
| Unix-socket file must be removed on clean shutdown | Prevents stale socket accumulation |
| TLS certificate hot-reload is deferred in Phase I | Certificate rotation requires a process restart |
| ADR-024 outbound model TLS note remains informational here | PI-15 does not enforce outbound `model_url` transport policy |
| `transport = "both"` must enforce HTTP bearer auth on the HTTP surface | The `"both"` mode does not relax HTTP auth |
| `POST /v1/interrupt` must return `404` for unknown task ids | Idempotent no-op is prohibited; `{"ok":false,"reason":"task_not_found"}` required |
| `POST /v1/approve` must return `404` for unknown task ids and `409` when no pending approval exists | Silent success on stale or unknown approval is prohibited |
| SSE keepalive comment must be emitted at least every 15 seconds during active turns | Prevents proxy/browser timeout disconnects |
| `/v1/schema` response is exempt from envelope validation | Pre-flight metadata, not a `RuntimeEnvelope` |
| ADR-024 reconciliation PR must be merged before PI-13 begins | PI-09–12 closeout owns the reconciliation; PI-13 may not start until ADR-024's checklist includes PI-09–16 |

---

## References

- `adr/ADR-025-runtime-json-handoff-contract.md` — canonical runtime JSON contract
- `adr/ADR-024-zero-licensing-cost-agent-parity-gaps.md` — Phase I reservation and sequencing gate
- `adr/ADR-023-deterministic-edit-loop.md` — edit-loop and validation behaviors
- `adr/completed/ADR-006-runtime-mode-contracts.md` — runtime seam contracts
