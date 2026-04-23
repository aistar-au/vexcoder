# ADR-042: Tool Registration and Approval Layer

**Status:** Accepted (amended 2026-04-07)  
**Chain:** ADR-029, ADR-040

## Context

Tool names varied across providers and local endpoints. Unknown tool names were silently ignored. Approval tracking was coupled to tool name rather than capability.

## Decision

- Local endpoint polling: `GET /props` or `GET /v1/models`; derive `max_tokens = server_n_ctx × 0.75`.
- Expose `n_ctx`, `n_batch`, model ID in telemetry summary line.
- Unknown tool names return a structured error; no silent discard.
- Multi-name alias registration: `run_command` (canonical schema), `run_shell_command`, `bash`, `execute_command`, `execute_bash` (aliases).
- Defense-in-depth approval chain: schema declaration → `tool_requires_confirmation()` → request approval → `ApprovalScope` tracking → sandbox wrapping.
- `ToolPolicy` enum: `Full` (all tools enabled), `Plan` (read-only), `Chat` (no tools).

## References

- [`serde_json`](https://docs.rs/serde_json) — tool schema serialization
- [RFC 7231 §4.3.1](https://www.rfc-editor.org/rfc/rfc7231#section-4.3.1) — HTTP GET semantics
