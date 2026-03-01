# Task DOC-03: ADR-022 Migration Guide

**Target File:** `docs/src/migration.md`, `docs/src/SUMMARY.md`

**ADR:** ADR-022 Phase 1

**Depends on:** CORE-15 (`test_config_loads_vex_model_name_without_claude_prefix` must be green)
**Parallel-safe with:** REF-09

---

## Issue

ADR-022 Phase 1 is a hard break for any deployment using legacy provider-branded
environment variable names. No migration documentation exists. Operators upgrading
an existing deployment have no reference for the rename table, the new policy file
format, or the changed validation rules.

---

## Decision

Create `docs/src/migration.md` containing:

1. A table mapping every removed legacy variable to its `VEX_*` replacement:

| Legacy variable | Replacement | Notes |
| :--- | :--- | :--- |
| `ANTHROPIC_API_URL` | `VEX_MODEL_URL` | No default; must be set explicitly |
| `ANTHROPIC_API_KEY` | `VEX_MODEL_TOKEN` | Optional for local runtimes |
| `ANTHROPIC_MODEL` | `VEX_MODEL_NAME` | No prefix requirement |
| `ANTHROPIC_VERSION` | — | Removed; protocol-internal |

2. Minimum working configuration examples for a local runtime and a self-hosted
   server, using only `VEX_*` names.

3. The `.vex/policy.toml` format with the full capability key list and value
   vocabulary (`"allow"`, `"deny"`, `"once"`, `"task"`, `"session"`).

4. A note that `Config::validate()` no longer enforces a vendor-specific model-name
   prefix and that any model identifier string is accepted.

5. Cross-links to ADR-022 phases and to the `TASKS/` manifests for each phase.

Add `migration.md` to `docs/src/SUMMARY.md` so it appears in the mdBook build.

---

## Definition of Done

1. `docs/src/migration.md` exists and contains the rename table, both config
   examples, the policy file format, and the prefix-removal note.
2. `docs/src/SUMMARY.md` includes a link to `migration.md`.
3. `mdbook build` completes without warnings on the docs directory.
4. No `ANTHROPIC_*` variable names appear as recommended values in the new doc.

---

## Anchor Verification

Review `docs/src/migration.md` and confirm:

- The rename table contains all four legacy variables.
- The local runtime example uses only `VEX_*` names.
- The policy file example lists all six `Capability` keys.
- `docs/src/SUMMARY.md` links to `migration.md`.

**What NOT to do:**
- Do not modify any `src/` Rust files in this task.
- Do not change existing ADR files.
- Do not add runtime behavior documentation to `migration.md` — scope is
  operator upgrade and config migration only.
