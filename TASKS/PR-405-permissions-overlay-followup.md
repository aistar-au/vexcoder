# PR 405 Follow-Up -- Permissions Overlay Policy

Branch: `work/vexcoder-privacy-policy-surface`

This note records the remaining operator-policy work adjacent to PR #405.
PR #405 adds a privacy disclosure surface. It does not add a new
permissions-mode enforcement layer, and it should not be read as doing so.

## Context

- PR #405 adds `vex privacy`, `GET /v1/privacy`, and the corresponding public
  documentation.
- The current tree already includes approval primitives, scoped grants,
  `/permissions`, `/allow`, `/deny`, and `ToolPolicy`-based schema shaping.
- `src/tools/operator/policy.rs` remains intentionally thin and currently wraps
  durable-access assertions rather than a full overlay-policy evaluator.
- A subsequent permissions overlay must remain distinct from the privacy
  disclosure surface and must remain distinct from `RuntimeCorePolicy`.
- The follow-up should be framed as an OWASP LLM06 control against excessive
  autonomy, with implementation invariants recorded before enforcement code
  lands.

## Follow-Up Scope

1. Draft a dedicated ADR before implementation. The ADR should extend the
   approval-first model recorded in `ADR-022`, the parity-gap tracking recorded
   in `ADR-024`, and the current approval-overlay baseline recorded in
   `ADR-042`.
2. Extend `src/tools/operator/policy.rs` into the canonical evaluation boundary
   for overlay-permission decisions rather than introducing a parallel operator
   policy surface.
3. Introduce a documented evaluation pipeline with fixed order:
   protected-path gate, deny rules, allow rules, and mode-based default.
4. Define the mode surface for `default`, `accept-edits`, `plan`, and
   `bypass-permissions` without weakening the existing approval guarantees.
5. Keep protected `.vex/` paths and credential-adjacent files outside any
   bypass override.
6. Demote untrusted workspaces to the default approval posture regardless of
   repo-local requests.
7. Keep non-interactive execution fail-closed when a requested action would
   otherwise require approval.
8. Define the introspection and evidence surfaces required to inspect the
   effective overlay policy before implementation begins.

## Invariants To Record Before Code Lands

- Protected-path checks run before allowlist or mode evaluation and remain
  binding in every mode.
- Deny rules take precedence over allow rules and mode defaults.
- Repo-local config may add protected paths but may not weaken the built-in
  protected set.
- Non-interactive execution returns an explicit refusal instead of silently
  approving a gated action.
- Effective settings resolve in a documented precedence order and may not be
  relaxed by untrusted workspace config.
- Permission decisions emit structured runtime evidence so audit and replay
  remain possible.

## Candidate Integration Points

- `src/tools/operator/policy.rs`
- `src/state/conversation/tools/`
- `src/config/load/`
- `src/app.rs` and `src/app/slash_commands.rs`
- `src/local_api.rs` and a possible read-only `/v1/permissions` metadata path

## Non-Goals For PR 405

- No permissions-mode enforcement changes in PR #405.
- No changes to the current approval-overlay semantics in PR #405.
- No protected-path rule engine in PR #405.
- No claim that the privacy disclosure surface substitutes for the permissions
  overlay lane.