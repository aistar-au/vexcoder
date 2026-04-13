# ADR-017: Append-Terminal Single Session Runtime

**Date:** 2026-02-21
**Status:** Superseded by ADR-018
**Deciders:** Core maintainer
**Related tasks:** (withdrawn after supersession)

## Context

The runtime had accumulated multiple UI execution paths (`App`, `TuiFrontend`,
`TtyApp`, and direct cli execution behavior), creating duplicate
state/render wiring and inconsistent prompt behavior.

## Decision

1. Use append-only cli session as the canonical runtime path for
   `cargo run`.
2. Remove parallel window-buffer app/frontend wrappers from runtime execution.
3. Keep runtime dispatch on `Runtime::run` with `FrontendAdapter<TuiMode>`.

## Consequences

- Simplified runtime surface and reduced duplicate UI paths.
- Improved cli prompt continuity for append-only sessions.

## Supersession Note

ADR-018 supersedes this direction by defining a managed-scrollback TUI as the
canonical runtime path once migration gates are complete.
