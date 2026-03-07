# Task PA-01: Layered Config Resolution Chain

**Target File:** `src/config.rs`, `tests/integration_test.rs`

**ADR:** ADR-024 Gap 3

**Depends on:** none

---

## Issue

Configuration still loads from environment variables only. ADR-024 requires a five-layer
resolution chain with deterministic precedence, strict TOML key validation, and an
explicit prohibition on loading `VEX_MODEL_TOKEN` from any config file.

---

## Decision

1. Extend `Config::load` with a layered resolver whose precedence is: environment,
   repo-local `.vex/config.toml`, user config, system config, then compiled defaults.
2. Bootstrap repo-local discovery from `std::env::current_dir()` by walking ancestors to
   the first directory containing `.vex/config.toml`. The resolved `working_dir` from the
   merged config may change runtime working directory after load; it must not be used to
   decide which repo-local config file is read.
3. Parse TOML into a typed config-layer struct whose accepted keys match the current
   neutral config surface. Unknown keys are hard failures. `model_token` in any config
   file is a hard failure with a diagnostic naming the offending file.
4. Missing files at any layer are ignored. Malformed TOML and invalid enum values are
   startup failures with file-path context.
5. Add a test-only load helper that accepts explicit repo/user/system paths so precedence
   can be verified without touching the operator's real home directory or `/etc`.

---

## Definition of Done

1. Environment values override repo-local, user, system, and defaults.
2. Repo-local config overrides user, system, and defaults.
3. User config overrides system and defaults.
4. `model_token` in any TOML config file is rejected.
5. Unknown TOML keys are rejected with a file-specific diagnostic.
6. `cargo test --all-targets` is green.

---

## Anchor Verification

`test_config_prefers_env_over_repo_user_system_and_defaults`

```rust
#[test]
fn test_config_prefers_env_over_repo_user_system_and_defaults() {
    let _lock = crate::test_support::ENV_LOCK.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let cwd = repo_root.join("nested/project");
    let user_cfg = temp.path().join("user-config.toml");
    let system_cfg = temp.path().join("system-config.toml");
    std::fs::create_dir_all(repo_root.join(".vex")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        repo_root.join(".vex/config.toml"),
        "model_name = \"repo-model\"\nmodel_url = \"http://repo.example/v1\"\n",
    )
    .unwrap();
    std::fs::write(&user_cfg, "model_name = \"user-model\"\n").unwrap();
    std::fs::write(&system_cfg, "model_name = \"system-model\"\n").unwrap();
    std::env::set_var("VEX_MODEL_NAME", "env-model");
    let cfg = Config::load_for_tests(&cwd, Some(&user_cfg), Some(&system_cfg)).unwrap();
    assert_eq!(cfg.model_name, "env-model");
    assert_eq!(cfg.model_url, "http://repo.example/v1");
    std::env::remove_var("VEX_MODEL_NAME");
}
```

**What NOT to do:**
- Do not read `VEX_MODEL_TOKEN` from any file-backed layer.
- Do not add provider-branded config names back into the runtime surface.
- Do not make config-file parse failures soft warnings.
- Do not modify `src/runtime/`, `src/state/`, or `src/api/` in this task.

---

## Dispatch Verification (dispatch only — implementation not yet landed)

### [PA-01] - Layered config resolution chain (dispatch only)

- Dispatcher: `dispatcher/adr-024-pa01-dispatch`
- Commit: `d3e8bf7b261702636d918121268297b95f5b39b7`
- Files changed:
  - `TASKS/PA-01-layered-config-resolution-chain.md` (+82 -0)
  - `TASKS/TASKS-DISPATCH-MAP.md` (+18 -2)
  - `TASKS/completed/REPO-RAW-URL-MAP.md` (+160 -159)
- Validation:
  - `cargo test test_config_prefers_env_over_repo_user_system_and_defaults --all-targets` : not valid yet (`0` tests matched; implementation not landed)
  - `cargo test --all-targets` : pass
  - `bash scripts/check_no_alternate_routing.sh` : pass
  - `bash scripts/check_forbidden_imports.sh` : pass
- Notes:
  - This branch stages the PA-01 dispatch manifest and map updates only.
  - Do not mark PA-01 green until the implementation branch lands the anchor test and it passes.

---

## Completion Verification

### [PA-01] - Layered config resolution chain

- Dispatcher: `dispatcher/adr-024-pa01-layered-config`
- Commit: `39d7ab385f5e8c53eac5b1e15a651eeb61c36dcc`
- Files changed:
  - `src/config.rs` (+323 -72)
  - `tests/integration_test.rs` (+110 -0)
- Validation:
  - `cargo test test_config_prefers_env_over_repo_user_system_and_defaults --all-targets` : pass
  - `cargo test --all-targets` : pass
  - `bash scripts/check_no_alternate_routing.sh` : pass
  - `bash scripts/check_forbidden_imports.sh` : pass
- Gate: commit-debug (2 providers: cerebras + google), 0 blocking findings; 5 findings auto-patched on first run (second run clean)
- Notes:
  - Layered config resolution implemented per ADR-024 Gap 3.
  - `model_token` remains environment-only.
  - Auto-patched: alias lists in TOML enum error messages; explicit validation of VEX_MODEL_BACKEND and VEX_TOOL_CALL_MODE env vars (both previously fell through silently).
