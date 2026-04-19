# ADR-003: Dual-protocol API client with URL-inferred protocol selection

**Date:** 2026-02-18  
**Status:** Accepted  
**Deciders:** Core maintainer  
**Related tasks:** None (original design decision; may spawn a FEAT task for additional backends)  
**Implemented in:** `src/api/client.rs` — protocol selection and chat-completions URL adaptation helpers

> Deprecated : the tagged-text fallback described in this ADR has been
> rewritten from current repository policy. ADR-047's API-first amendment and
> PR #408 remove downstream tagged/XML compatibility repair in favor of
> API-boundary normalization plus explicit recoverable errors for
> non-canonical provider payloads.

---

## Context

`vexcoder` targets two distinct user groups:

1. **Remote API users** who connect to a hosted `messages-v1` endpoint (`/v1/messages`). This group uses hosted models and expects streaming SSE with `content_block_start` / `content_block_delta` events and native `tool_use` blocks.

2. **Local model users** who run a local model server. These tools commonly expose either a `messages-v1` endpoint or a chat-compatible `/v1/chat/completions` endpoint. This group wants the same agentic loop with local models regardless of which wire format is configured.

Requiring users to configure a protocol enum explicitly would create friction. Most users know their endpoint URL but do not necessarily know which wire protocol it speaks.

Additionally, tool call representation differs between protocols: the `messages-v1` path uses structured `tool_use` content blocks; the chat-completions protocol uses `tool_calls` arrays in assistant deltas. The stream parser and message history builder must handle both.

A tagged-text fallback (`<function=name><parameter=key>value</parameter></function>`) also exists for local models that do not support either native tool protocol reliably.

---

## Decision

Implement a single `ApiClient` that supports both `messages-v1` and chat-compatible protocol modes, with explicit configuration taking precedence and URL adaptation limited to filling in the endpoint path that matches the configured protocol.

**Protocol inference rules** (`infer_api_protocol()`):
- URL contains `/chat/completions` → chat-completions protocol
- URL contains `/messages` → `messages-v1` protocol (covers both `/v1/messages` and the transposed `/messages/v1`)
- URL ends with `/v1` → chat-completions protocol (base path convention)
- Anything else → `messages-v1` (default)

**URL adaptation**:
- `messages-v1` mode maps `/v1` → `/v1/messages`
- chat-compatible mode maps `/v1` → `/v1/chat/completions`
- explicit `/messages` and `/chat/completions` URLs are preserved for their configured protocol
- transposed `/messages/v1` is rewritten to the accepted `/v1/messages` or `/v1/chat/completions` form

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
