# ADR-028: Application facade and transport boundaries

**Date:** 2026-03-15
**Status:** Active — Phase 1, 2, and transport extraction committed 2026-03-25
**Deciders:** Core maintainer
**Location:** `adr/ADR-028-application-facade-and-transport-boundaries.md`
**Amends:** ADR-018, ADR-019, and follow-up runtime/TUI cutover ADRs
**Related:** ADR-024 (Phase I reservation), ADR-025 (runtime JSON handoff contract), ADR-026 (LocalApiServer transport binding), ADR-027 (full-screen TUI command-session capture), ADR-006 (runtime mode contracts)

---

## Definitions

- **Application facade** — a transport-agnostic application API that accepts high-level commands and exposes runtime-facing results and events to outer layers.
- **Transport** — the network or IPC layer that binds sockets, frames messages, and forwards facade output over concrete protocols such as HTTP, SSE, or line-delimited local socket streams.
- **Orchestration / agent loop** — the iterative runtime loop that reads model output, detects tool calls, executes tools, merges results, and emits structured engine events.
- **Runtime core engine** — deterministic state transitions, validation, policy checks, and tool plumbing; no transport or UI concerns.

---

## Context

`src/app.rs` and `src/bin/vex.rs` currently sit close to several concerns at once: TUI command handling, runtime coordination, startup wiring, and user-facing entrypoint behavior. The repository already reserves a `LocalApiServer` path in ADR-024 and, through ADR-025 and ADR-026, now has a canonical runtime JSON contract plus a transport-binding ADR for that local API surface.

What is still missing is the explicit boundary that says:

- what the application layer owns,
- what the transport layer owns,
- what the CLI binary owns,
- and how those outer layers are prevented from reaching directly into runtime internals.

Without that boundary, `src/app.rs` risks remaining the convergence point for unrelated concerns, and future transports could bypass the same canonical seam that ADR-025 and ADR-026 were introduced to protect.

---

## Relationship to ADR-025 and ADR-026

ADR-028 does not replace ADR-025 or ADR-026.

- **ADR-025** remains the canonical machine-readable runtime contract (`RuntimeRequest`, `RuntimeEnvelope`, normalization, schema, grammar).
- **ADR-026** remains the LocalApiServer transport-binding ADR (HTTP, SSE, Unix socket, auth, `/v1/schema`).
- **ADR-028** defines the module boundaries and dependency direction by which CLI and transport layers reach that canonical runtime contract through an explicit application facade.

The facade must not invent a second canonical event schema. Where the facade needs machine-readable output, it uses ADR-025 `RuntimeEnvelope` directly or wraps it only in transport-local framing.

---

## Decision

Adopt a layered boundary model with strict inward dependency direction.

### 1. Transport layer

**Target modules**

- `src/server.rs`
- `src/server/http.rs`
- `src/server/sse.rs`
- `src/server/socket.rs`
- `src/server/handlers.rs`
- `src/server/util.rs`

**Responsibilities**

- accept inbound local connections and manage lifecycle;
- bind and listen on configured local transports authorized by ADR-026;
- expose minimal metadata or readiness endpoints such as `/v1/health`;
- frame facade output for concrete transports such as SSE or line-delimited local-socket JSON;
- manage backpressure and slow-client handling at the transport boundary.

**Non-responsibilities**

- no runtime logic, tool semantics, task orchestration, or provider protocol parsing;
- no alternate canonical event schema;
- no direct dependency on runtime internals bypassing the facade.

**Scope note**

For the current ADR chain, transport means the ADR-026 `LocalApiServer` surface only: loopback HTTP plus Unix-domain socket. Future transport families such as WebSocket, gRPC, TLS termination, or remote-serving surfaces remain separate follow-up decisions and are not authorized by ADR-028 alone.

### 2. Application facade layer

**Target modules**

- `src/app.rs`                    # module root during transition
- `src/app/core.rs`
- `src/app/commands.rs`
- `src/app/context.rs`
- `src/app/errors.rs`
- `src/app/util.rs`

**Responsibilities**

- expose a stable, transport-agnostic application API for CLI and server callers;
- receive high-level commands from outer layers and route them into runtime entrypoints;
- centralize command semantics and validation that should be shared across CLI and server;
- shape application output into ADR-025 canonical runtime JSON where a machine-readable seam is required;
- provide a narrow coordination boundary above runtime and below CLI or transport.

**Non-responsibilities**

- no HTTP, SSE, or socket implementation;
- no terminal rendering;
- no provider wire parsing;
- no second canonical event-envelope model separate from ADR-025.

**Facade API sketch**

```rust
// src/app.rs

pub async fn execute_command(req: CommandRequest) -> Result<CommandResponse, AppError>;
pub fn subscribe_runtime_events() -> impl futures_core::Stream<Item = RuntimeEnvelope>;
pub async fn shutdown_gracefully() -> Result<(), AppError>;
```

**Facade rule**

When machine-readable event streaming is needed, the facade emits ADR-025 `RuntimeEnvelope`. Transport layers may frame those envelopes, but they must not replace them with a facade-only event format.

### 3. Orchestration / agent-loop layer

**Target modules**

- `src/runtime/edit_loop.rs`
- `src/runtime/loop.rs`
- `src/runtime/context.rs`
- `src/runtime/context_assembler.rs`
- `src/runtime/task_state.rs`

**Responsibilities**

- run the iterative agent loop;
- advance turns deterministically;
- normalize provider and runtime-native events into canonical runtime events as required by ADR-025;
- expose runtime entrypoints consumed by the facade.

**Non-responsibilities**

- no transport framing;
- no CLI parsing;
- no terminal rendering ownership;
- no direct transport or server dependency.

### 4. Runtime core engine

**Target scope**

- lower-level `src/runtime/*` primitives beneath orchestration entrypoints

**Responsibilities**

- deterministic state transitions;
- validation and policy checks;
- tool execution plumbing;
- context assembly inputs and outputs;
- core state management.

**Non-responsibilities**

- no HTTP, SSE, or socket behavior;
- no CLI or TUI policy ownership beyond reusable primitives;
- no direct dependency on facade or transport layers.

### 5. CLI layer

**Target module**

- `src/bin/vex.rs`

**Responsibilities**

- parse CLI arguments;
- load configuration;
- select startup routing (`TuiMode`, `BatchMode`, facade-mediated API path, resume and print flows);
- call the application facade for shared command semantics or machine-readable interaction paths.

**Non-responsibilities**

- no transport framing;
- no provider protocol parsing;
- no ownership of reusable runtime command semantics;
- no direct long-term dependence on runtime internals where the facade provides the intended boundary.

**Thin-wrapper rule**

`src/bin/vex.rs` remains the CLI entrypoint, but it must be a thin wrapper around config loading, startup routing, and facade invocation. It must not become the long-term home of shared application or transport logic.

---

## Dependency direction

**Allowed**

- `CLI -> Application facade`
- `Transport -> Application facade`
- `Application facade -> Orchestration / Runtime`
- `Orchestration -> Runtime core`

**Forbidden**

- `Runtime -> CLI`
- `Runtime -> Transport`
- `Runtime -> terminal UI`
- `Application facade -> HTTP/SSE/socket internals`
- `Application facade -> CLI`
- `Transport -> Runtime` except through facade-owned entrypoints

---

## Canonical event rule

ADR-025 `RuntimeEnvelope` is the canonical runtime event contract.

ADR-028 therefore imposes the following rule:

- the facade may expose Rust-native application result types for local in-process use;
- the facade may expose ADR-025 `RuntimeEnvelope` for machine-readable event streams;
- neither the facade nor any transport layer may invent a second canonical event envelope that competes with ADR-025.

This prevents duplication between provider-native events, facade-local ad hoc events, transport-local chunks, and canonical runtime JSON.

---

## Consequences

**Positive**

- `src/app.rs` no longer remains the convergence point for unrelated concerns.
- `src/bin/vex.rs` becomes a true thin wrapper around routing and facade invocation.
- transport can evolve within the bounds of future ADRs without contaminating runtime.
- command semantics become reusable across CLI and LocalApiServer.
- architecture becomes easier to test, reason about, and extend.

**Negative**

- module moves and compatibility shims are required during migration.
- some TUI-centric behavior currently living near app wiring will need relocation.
- short-term churn in imports and tests is expected as code is decomposed.

---

## Non-goals

This ADR intentionally does not define:

- the exact LocalApiServer schema or final SSE framing details beyond ADR-026;
- a second facade-specific event envelope;
- whether CLI will permanently self-client or keep a direct facade path for some local flows;
- gRPC, WebSocket, TLS, or remote-serving transport details;
- a commit-by-commit migration plan.

Those remain follow-up implementation or transport decisions.

---

## Migration guidance

Use the following order to minimize breakage.

1. **Create the application facade skeleton** under `src/app/` while keeping `src/app.rs` as the module root during transition. Define facade entrypoints plus the shared error and command types. Reuse ADR-025 `RuntimeEnvelope` for machine-readable event streaming rather than introducing a new canonical envelope.
2. **Refactor `src/app.rs`** by moving shared application coordination and command semantics into facade modules. Keep behavior identical. Use compatibility shims while the cutover is in progress.
3. **Reduce `src/bin/vex.rs`** to CLI parsing, config loading, startup routing, and facade calls. Do not remove legitimate startup-routing responsibilities, but do remove reusable application semantics from the binary.
4. **Introduce `src/server/` submodules** only for the ADR-026-authorized local transports. Server modules consume facade output and frame ADR-025 envelopes for transport. *(Completed 2026-03-25: `src/server/mod.rs`, `http.rs`, `sse.rs`, `socket.rs`, `handlers.rs`, `util.rs` extracted from `src/local_api.rs`.)*
5. **Keep direct facade invocation available during transition** for local CLI paths if needed. A self-client or embedded-server path may be introduced later, but it is not required by ADR-028 itself.
6. **Tighten dependency boundaries** with tests and grep-based contract checks so runtime does not reach outward into CLI or transport. *(Completed 2026-03-25: `tests/dependency_direction_tests.rs` enforces 10 boundary rules including server module existence and facade routing.)*

**Backpressure and slow-client policy**

Transport implementations must use bounded buffers and define a clear slow-client policy so the runtime is not blocked indefinitely by slow consumers. The precise policy is transport-specific and belongs to implementation or transport-specific ADRs, not to the facade contract itself.

**Testing requirements**

- unit tests for facade functions and command routing;
- tests that facade machine-readable streams emit ADR-025 `RuntimeEnvelope`;
- integration tests that confirm transport code reaches runtime only through facade entrypoints;
- ADR-026 integration tests remain the source of truth for LocalApiServer framing, auth, and ordering behavior.

**Migration safety**

Use compatibility shims or feature-gated cutover points for one release window if needed. Remove transitional shims once facade and transport boundaries are stable.

---

## ADRs to update

Update the following ADRs to reflect the explicit facade and transport split.

- **ADR-018** — remove wording that can be read as permission for the long-term app layer to continue mixing TUI, runtime coordination, command semantics, and startup wiring.
- **ADR-019 and ADR-027 follow-up runtime/TUI cutover notes** — clarify that `src/bin/vex.rs` is a thin CLI wrapper and that machine-readable runtime output is facade-facing rather than TUI-shaped output.
- **Any ADR that discusses embedded API or server startup** — clarify that transport wraps facade output and does not reach directly into runtime internals.

**Minimal correction rule**

Where older ADRs blur distinctions, update them to reflect:

- application facade vs transport,
- canonical runtime envelopes vs transport frames,
- CLI wrapper vs frontend implementation,
- and facade-mediated access to runtime.

---

## Debug fixes recorded 2026-03-17

Three bugs surfaced during review of the Phase 1 / Phase 2 branch series
(`work/vexcoder-adr-028-phase1-facade-skeleton`,
`work/vexcoder-adr-028-phase2-tui-latency-facade`).  Each is patched
in the debug commit on this date and recorded here for traceability.

### Bug 1 — local protocol routing mismatch

**Location:** `src/api/client.rs` — `should_prefer_chat_compat_wire_protocol()`

**Root cause:** The client mixed explicit protocol selection with URL heuristics.
An explicit `messages-v1` configuration could still be redirected to the
chat-compatible path, which made the wire contract harder to reason about and
hid the real endpoint being used.

**Fix:** Protocol selection now follows the configured protocol exactly.
Explicit `messages-v1` requests use `/messages`; explicit chat-compatible
requests use `/chat/completions`.  Bare `/v1` base URLs are only expanded into
the matching endpoint path for the configured protocol.

**Tests added:**
- `test_local_messages_endpoint_keeps_messages_v1_wire_protocol`
- `test_local_bare_v1_endpoint_resolves_messages_v1_url`
- `test_local_bare_v1_endpoint_resolves_chat_compat_url`

### Bug 2 — TUI must own the alternate-screen session surface

**Location:** `src/terminal.rs`

**Root cause:** The terminal lifecycle notes drifted into contradictory wording.
The interactive task surface is a fullscreen session and therefore must own the
alternate screen buffer consistently instead of mixing primary-surface wording
with alternate-screen enter/leave calls.

**Fix:** `enter_full_screen_mode()` now executes `EnterAlternateScreen` before
`EnableBracketedPaste`.  `restore()` now executes `LeaveAlternateScreen` before
`Show`, cleanly returning the user to their pre-session terminal state. The
fullscreen task surface keeps its own transcript scroll model inside the
session; host-terminal PageUp after exit shows the pre-session shell history,
not the in-session transcript.

### Bug 3 — Orchestrator activity pane does not show live steps

**Location:** `src/app/layout.rs` (`task_activity_rows`), `src/ui/render.rs`
(`render_task_layout`, `pipeline_activity_line`)

**Root cause:** `task_activity_rows()` only iterated `current_turn_tool_invocations`
(completed calls).  In-flight tool calls stored in `pending_turn_tool_calls`
were invisible: the activity pane went blank while a tool was executing.  The
row list was also uncapped (up to 8 fallback history lines with no upper bound
during active turns).  `render_task_layout` rendered activity rows as
monochrome `Line::from` strings with no structured prefix styling.

**Fix:**
- `task_activity_rows()` now appends in-flight calls from
  `pending_turn_tool_calls` with `[->] name: running…` prefix.
- The list is capped at `MAX_ACTIVITY_ROWS = 6` for a stable 6-line pipeline
  dropdown appearance.
- Completed calls use `[ok]` / `[!]` prefixes matching existing render colours.
- Added `pipeline_activity_line()` helper that splits each row into a bold
  coloured prefix `Span` and a body `Span`, giving the orchestration view a
  structured live-pipeline appearance without copying proprietary CLI tool
  names or logos.
- `render_task_layout` activity title now reflects state: "Orchestrating" when
  in-flight steps exist, "Steps" otherwise.

---

## References

- `adr/completed/ADR-006-runtime-mode-contracts.md` — runtime seam contracts
- `adr/ADR-024-zero-licensing-cost-agent-parity-gaps.md` — LocalApiServer reservation and Phase I sequencing guard
- `adr/ADR-025-runtime-json-handoff-contract.md` — canonical runtime JSON contract
- `adr/ADR-026-localapiserver-transport-binding.md` — LocalApiServer transport binding
- `adr/ADR-027-full-screen-tui-command-session-capture.md` — current full-screen TUI and command-session capture behavior
- `../vexdraft/scripts/commit-debug.py` — authoritative cross-repo debug commit script
