# ADR-003: Dual-protocol API client with URL-inferred protocol selection

**Date:** 2026-02-18  
**Status:** Accepted  
**Deciders:** Core maintainer  
**Related tasks:** None (original design decision; may spawn a FEAT task for additional backends)  
**Implemented in:** `src/api/client.rs` — protocol selection and chat-completions URL adaptation helpers

---

## Context

`vexcoder` targets two distinct user groups:

1. **Remote API users** who connect to a hosted `messages-v1` endpoint (`/v1/messages`). This group uses hosted models and expects streaming SSE with `content_block_start` / `content_block_delta` events and native `tool_use` blocks.

2. **Local model users** who run llama.cpp server, Ollama, or LM Studio locally. These tools expose a chat-completions-compatible `/v1/chat/completions` endpoint. This group cannot use the `messages-v1` protocol but wants the same agentic loop with local models.

Requiring users to configure a protocol enum explicitly would create friction. Most users know their endpoint URL but do not necessarily know which wire protocol it speaks.

Additionally, tool call representation differs between protocols: the `messages-v1` path uses structured `tool_use` content blocks; the chat-completions protocol uses `tool_calls` arrays in assistant deltas. The stream parser and message history builder must handle both.

A tagged-text fallback (`<function=name><parameter=key>value</parameter></function>`) also exists for local models that do not support either native tool protocol reliably.

---

## Decision

Implement a single `ApiClient` that internally selects between the `messages-v1` and chat-completions protocol modes based on the endpoint URL, with a manual override via `VEX_API_PROTOCOL`.

**Protocol inference rules** (`infer_api_protocol()`):
- URL contains `/chat/completions` → chat-completions protocol
- URL ends with `/v1` → chat-completions protocol (base path convention)
- Anything else → `messages-v1` (default)

**URL adaptation** (chat-completions URL adaptation helper):
- `/v1/messages` → `/v1/chat/completions`
- `/v1` → `/v1/chat/completions`
- Already correct → unchanged

**Stream parser** (`src/api/stream.rs`): attempts `messages-v1` SSE parse first; on failure attempts chat-completions chunk parse. Chat-completions tool calls are translated into the same `StreamEvent` enum used by the `messages-v1` path, so `ConversationManager` is protocol-agnostic above the stream layer.

**Tagged-text fallback**: if neither protocol produces tool use blocks, `parse_tagged_tool_calls()` scans the assistant text for `<function=name>` syntax. This provides compatibility with models that emit tool calls as formatted text rather than structured JSON.

---

## Rationale

URL-based inference requires no configuration change for the common case. A user switching from one hosted `messages-v1` endpoint to `http://localhost:8080/v1/chat/completions` gets the correct protocol automatically. The override variable exists for edge cases (e.g., a proxy that serves the `messages-v1` protocol on a non-standard URL).

Translating chat-completions events to the shared `StreamEvent` enum — rather than having two parallel code paths in `ConversationManager` — keeps the conversation logic in one place. The translation cost is minimal and contained to `stream.rs`.

The tagged-text fallback preserves compatibility with older or constrained local models without requiring the operator to configure anything. It is purely additive.

---

## Alternatives considered

### Separate protocol-specific client types

Cleaner at the type level but forces `ConversationManager` to accept a trait object, introducing dynamic dispatch and lifetime complexity. The unified `ApiClient` with an internal enum achieves the same separation with less indirection.

### User-configured protocol enum in config file

More explicit but creates friction. The most common migration path (hosted `messages-v1` endpoint → local model) requires the user to edit two fields instead of one.

### Chat-completions protocol only, with a `messages-v1` adapter

Would lose native `messages-v1` features (extended thinking, `betas` headers, native tool_choice) that do not map cleanly to the chat-completions schema.

---

## Consequences

**Easier:**
- Zero-config local model support. Point the configured model URL at any chat-completions-compatible server and it works.
- `ConversationManager` is protocol-agnostic; no protocol logic leaks into the conversation layer.
- The tagged fallback means even models that ignore the tools schema can participate in the agentic loop.

**Harder:**
- The stream parser is more complex: it attempts two parse strategies per SSE event. Parse errors from the `messages-v1` path are silently retried as chat-completions protocol before being logged.
- Testing requires mock streams for both protocols (see `src/api/mock_client.rs`).
- Adding a third protocol requires extending the enum, the inference logic, the URL adapter, and the stream parser. The abstraction is extensible but not free.

**Constraints imposed on future work:**
- Protocol selection must remain automatic (URL-inferred) for the common case. Do not make `VEX_API_PROTOCOL` required.
- New protocol-specific features (e.g., `messages-v1` extended thinking) must degrade gracefully when the active protocol is chat-completions based.
- All protocol paths must be covered by integration tests using `MockApiClient`. Adding a new protocol path without mock coverage is not acceptable.
