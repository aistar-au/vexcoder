# Task CORE-15: Neutral Config Cutover

**Target File:** `src/config.rs`, `tests/integration_test.rs`

**ADR:** ADR-022 Phase 1

**Depends on:** —
**Parallel-safe with:** DOC-03

---

## Issue

`src/config.rs` reads `ANTHROPIC_API_URL`, `ANTHROPIC_API_KEY`, `ANTHROPIC_MODEL`, and
`ANTHROPIC_VERSION`. It carries a hardcoded provider URL default and enforces a `claude-`
model-name prefix in `Config::validate()` for non-local endpoints. The `anthropic_version`
struct field is emitted as an HTTP request header. None of this is neutral.

ADR-022 Phase 1 requires all of this to be replaced with the normative `VEX_*` surface
before any other phase work begins.

---

## Decision

1. Replace all four `std::env::var("ANTHROPIC_*")` reads with `VEX_MODEL_URL`,
   `VEX_MODEL_TOKEN`, `VEX_MODEL_NAME`. Drop the version read entirely.
2. Rename struct fields: `api_url` → `model_url`, `api_key` → `model_token`,
   `model` → `model_name`. Remove `anthropic_version` field.
3. Remove the hardcoded `https://api.anthropic.com/v1/messages` default.
   No branded default is permitted. Require `VEX_MODEL_URL` to be set explicitly,
   or default to `http://localhost:11434/v1` for local-runtime parity.
4. Replace the `claude-` prefix enforcement in `Config::validate()` with a neutral
   non-empty check. The `local/` prefix guard for non-local endpoints is retained.
5. Update `tests/integration_test.rs` to use `VEX_*` var names.

---

## Definition of Done

1. `src/config.rs` contains no `ANTHROPIC_*` variable names, no branded defaults,
   and no `claude-` prefix check.
2. `anthropic_version` field does not exist anywhere in `src/config.rs`.
3. `Config::validate()` passes for `VEX_MODEL_NAME=llama-3` on a non-local endpoint.
4. `Config::validate()` passes for a local endpoint with no token set.
5. `cargo test --all-targets` is green.

---

## Anchor Verification

`test_config_loads_vex_model_name_without_claude_prefix`

```rust
#[test]
fn test_config_loads_vex_model_name_without_claude_prefix() {
    let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
    std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
    std::env::set_var("VEX_MODEL_NAME", "llama-3-70b");
    std::env::remove_var("VEX_MODEL_TOKEN");
    let cfg = Config::load().expect("load failed");
    assert!(cfg.validate().is_ok(), "neutral model name must pass validation");
    std::env::remove_var("VEX_MODEL_URL");
    std::env::remove_var("VEX_MODEL_NAME");
}
```

**What NOT to do:**
- Do not retain any `ANTHROPIC_*` variable read as a fallback alias.
- Do not add new branded defaults under a different name.
- Do not touch `src/api/client.rs` header emission in this task — that is REF-09.
- Do not modify `src/tools/`, `src/state/`, or `src/runtime/policy.rs`.
