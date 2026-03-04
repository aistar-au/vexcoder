# Task REF-09: Model Backend Seam

**Target File:** `src/api/client.rs`, `src/config.rs`, new `src/runtime/backend.rs`

**ADR:** ADR-022 §Normative Config and Interface Changes

**Depends on:** CORE-15 (`test_config_loads_vex_model_name_without_claude_prefix` must be green)
**Parallel-safe with:** DOC-03

---

## Issue

`ApiClient` in `src/api/client.rs` is shaped around a single provider's wire protocol.
It emits an `anthropic-version` request header unconditionally. The `ApiProtocol` enum
is provider-adjacent. No neutral `ModelBackend` abstraction exists.

ADR-022 requires a neutral `ModelBackend` trait that abstracts all model communication
and internalizes version-header negotiation per `ModelProtocol` variant so it is no
longer a user-facing config concern.

---

## Decision

1. Introduce `ModelBackendKind` and `ModelProtocol` enums in `src/runtime/backend.rs`
   as specified in ADR-022.
2. Introduce a `ModelBackend` trait in the same file with `backend_kind()`, `protocol()`,
   `is_local()`, and `async fn create_stream(...)`.
3. Move version-header emission from `ApiClient::create_stream()` into a
   `ModelProtocol::request_headers()` helper so it is protocol-internal.
4. Parse `VEX_MODEL_BACKEND` and `VEX_MODEL_PROTOCOL` from `Config` and wire through
   to the backend selection path.
5. Existing `ApiClient` can implement `ModelBackend` as a concrete type; `MockApiClient`
   likewise.

---

## Definition of Done

1. `ModelBackend`, `ModelBackendKind`, and `ModelProtocol` compile from
   `src/runtime/backend.rs`.
2. `anthropic-version` header emission does not appear as a hardcoded literal outside
   a `ModelProtocol` variant handler.
3. `VEX_MODEL_BACKEND` env var is parsed by `Config::load()`.
4. Existing streaming and mock tests remain green.
5. `cargo test --all-targets` is green.

---

## Anchor Verification

`test_model_backend_kind_parses_from_env_var`

```rust
#[test]
fn test_model_backend_kind_parses_from_env_var() {
    let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
    std::env::set_var("VEX_MODEL_BACKEND", "local-runtime");
    std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
    std::env::set_var("VEX_MODEL_NAME", "local-model");
    let cfg = Config::load().expect("load failed");
    assert!(cfg.validate().is_ok());
    assert_eq!(cfg.model_backend, ModelBackendKind::LocalRuntime);
    std::env::remove_var("VEX_MODEL_BACKEND");
    std::env::remove_var("VEX_MODEL_URL");
    std::env::remove_var("VEX_MODEL_NAME");
}
```

**What NOT to do:**
- Do not delete `ApiClient` — wrap it behind the trait, do not replace it entirely.
- Do not change tool schemas or system prompt in this task.
- Do not touch `src/tools/`, `src/state/`, `src/app.rs`, or `src/runtime/policy.rs`.
- Do not introduce new `UiUpdate` variants.
