# ADR-022 Amendment (2026-03-03)

**Status:** Amended  
**Amends:** ADR-022

## Amendment

- Lock Phase 1 command-execution scope to one-shot with captured output; interactive PTY deferred to Phase 3.
- `run_command` schema uses `command` (string) and `args` (array of strings); no shell expansion at the tool boundary.
- `CommandRunner` returns `CommandOutput { stdout, stderr, exit_code }` only; the model receives the struct, not a combined string.
