# ADR-022 Migration Guide

This guide covers the breaking configuration changes in ADR-022 Phase 1.
All legacy provider-branded environment variables have been removed and
replaced with neutral `VEX_*` names.

## Environment Variable Rename Table

| Legacy variable     | Replacement       | Notes                              |
| :------------------ | :---------------- | :--------------------------------- |
| `ANTHROPIC_API_URL` | `VEX_MODEL_URL`   | Defaults to `http://localhost:11434/v1` when unset |
| `ANTHROPIC_API_KEY` | `VEX_MODEL_TOKEN` | Optional for local runtimes        |
| `ANTHROPIC_MODEL`   | `VEX_MODEL_NAME`  | No prefix requirement              |
| `ANTHROPIC_VERSION` | —                 | Removed; protocol-internal         |

## Minimum Working Configurations

### Local runtime (e.g. Ollama, llama.cpp, LM Studio)

```sh
export VEX_MODEL_NAME=llama3
# Optional: override default local endpoint
# export VEX_MODEL_URL=http://localhost:11434/v1
# Optional token for gateways that require auth
# export VEX_MODEL_TOKEN=your-token-here
```

When `VEX_MODEL_URL` is unset, `vexcoder` uses `http://localhost:11434/v1`.
Set it explicitly when your local runtime listens on a different endpoint.

### Self-hosted server

```sh
export VEX_MODEL_URL=https://model.example.internal/v1
export VEX_MODEL_NAME=mistral-7b-instruct
export VEX_MODEL_TOKEN=your-token-here
```

## Capability Policy Format

Create `.vex/policy.toml` in your project root to override the default
approval posture. All six capability keys must use one of:
`"allow"`, `"deny"`, `"once"`, `"task"`, `"session"`.

```toml
# .vex/policy.toml
[capabilities]
ReadFile   = "allow"
WriteFile  = "once"
ApplyPatch = "once"
RunCommand = "task"
Network    = "deny"
Browser    = "deny"
```

## Validation Changes

`Config::validate()` no longer enforces a vendor-specific model-name prefix.
Any non-empty model identifier string is accepted. The `local/` prefix guard
for non-local endpoints is retained.

## Cross-references

- Architecture decision: [ADR-022 roadmap](../../TASKS/ADR-022-free-open-coding-agent-roadmap.md) — Phase 1
- Config cutover implementation: [CORE-15](../../TASKS/CORE-15-neutral-config-cutover.md)
- This guide's task manifest: [DOC-03](../../TASKS/DOC-03-adr-022-migration-guide.md)
