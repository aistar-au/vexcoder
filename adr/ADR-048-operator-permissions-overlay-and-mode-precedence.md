# ADR-048: Operator Permissions Overlay and Mode Precedence

- **Status:** Proposed
- **Date:** 2026-04-19
- **Deciders:** Core maintainer
- **Depends on:** ADR-022, ADR-024 Gap 13, ADR-042, ADR-038
- **Supersedes:** None
- **Superseded by:** None

## Context

`vexcoder` already has the correct approval-first foundation for the first
release, but the current tree does not yet define a single operator-policy
engine for overlay permission modes. The existing pieces are intentionally
separate:

- ADR-022 defines capability-based approval and keeps that lane distinct from
  `RuntimeCorePolicy`.
- ADR-024 Gap 13 defines the interactive `/permissions`, `/allow`, and `/deny`
  command surface for the current in-memory grant set.
- ADR-042 defines the schema-facing `ToolPolicy` gate and the current
  defense-in-depth approval overlay for `run_command`.
- `src/tools/operator/policy.rs` currently enforces only durable `.vex/`
  access constraints introduced by ADR-038.

PR #405 added a privacy disclosure surface through `vex privacy` and
`GET /v1/privacy`. That disclosure lane is correct, but it must remain distinct
from the permissions-overlay lane. Privacy disclosure informs the operator
about existing boundaries. Permissions overlay controls runtime autonomy and
tool authority. Treating them as the same control would blur user
participation and security into one surface and would make both harder to
audit.

This ADR records the pre-implementation invariants for the later permissions
overlay. The framing is OWASP LLM06 Excessive Agency: the runtime must keep
tool autonomy, approval policy, and protected local state under explicit,
observable control before enforcement code lands.

## Decision

### D1: Keep the permissions overlay distinct from privacy disclosure and `RuntimeCorePolicy`

The permissions overlay is a runtime enforcement surface. It is not a privacy
policy, and it does not replace `RuntimeCorePolicy`.

- Privacy disclosure continues to describe storage, transport, credential,
  telemetry, and retention behaviour.
- `RuntimeCorePolicy` continues to shape prompt and evidence handling.
- The permissions overlay governs whether a requested action is allowed,
  denied, or requires explicit operator approval.

No implementation lane may claim that `vex privacy`, `/permissions`, or
`ToolPolicy` alone already provides the full permissions overlay.

### D2: `src/tools/operator/policy.rs` is the reserved evaluation boundary

The operator-policy module is the reserved home for the later overlay
evaluator. The current durable-access helpers remain in place, but future mode
evaluation, protected-path checks, and precedence rules must grow at this seam
rather than being duplicated across UI handlers, tool dispatch helpers, or
transport endpoints.

This decision preserves one review boundary for operator policy while keeping
durable `.vex/` access assertions and later permissions-overlay decisions
adjacent instead of fragmented.

### D3: The overlay exposes four permission modes

The later implementation must expose the following mode surface:

```rust
pub enum PermissionMode {
    Default,
    AcceptEdits,
    Plan,
    BypassPermissions,
}

pub enum PermissionDecision {
    Allow,
    Deny,
    Prompt,
    AllowSandboxed,
}
```

- `Default`: approval-first posture.
- `AcceptEdits`: edit-focused fast path without removing protection for
  commands, protected paths, or policy-denied actions.
- `Plan`: read-oriented mode that suppresses mutating autonomy.
- `BypassPermissions`: broad auto-approval mode for eligible actions, but still
  subordinate to protected-path and deny-rule checks.

`ToolPolicy` remains a schema and dispatch visibility concern. It does not
become a synonym for `PermissionMode`.

### D4: Evaluation order is fixed and documented

The overlay evaluator must apply the following order with no hidden bypasses:

1. Protected-path gate.
2. Deny rules.
3. Allow rules.
4. Mode-based default.

This order is normative. The runtime must not allow a mode override or an
allow rule to weaken an earlier protected-path or deny decision.

### D5: Protected paths remain binding in every mode

The built-in protected set includes `.vex/state/`, `.vex/index/`, the main
`.vex/` configuration surface, and credential-adjacent files used by the local
runtime.

- The built-in protected set is not removable by config.
- Config may add protected paths, but may not relax the built-in set.
- `BypassPermissions` does not override protected-path denials.

This rule keeps local control surfaces and durable state outside permissive
mode shortcuts.

### D6: Settings precedence is explicit, and untrusted workspaces are demoted

The effective overlay policy must resolve in this order:

1. Enterprise-managed settings
2. User settings
3. Trusted project settings
4. Built-in defaults

Repo-local settings from an untrusted workspace must be demoted to the default
approval posture. An untrusted repository must not be able to raise its own
autonomy level by committing a more permissive overlay mode.

### D7: Interactive overlay prompts are non-blocking, and non-interactive mode fails closed

Interactive sessions may present approval notices through the existing overlay
surface. Those notices remain informative rather than silent auto-approvals.

In non-interactive execution, any action that would require an operator prompt
must return an explicit refusal. The runtime must not silently approve a gated
action merely because no interactive prompt is available.

### D8: Introspection and evidence are required before enforcement lands

The overlay lane must expose its effective state in operator-visible form
before broad enforcement work lands.

Required surfaces:

- interactive inspection through `/permissions`
- non-interactive or external inspection through a read-only metadata surface
  such as `vex permissions` or `GET /v1/permissions`
- structured runtime evidence for allow, deny, and prompt decisions

The evidence surface must record whether a decision came from protected-path
policy, deny rule, allow rule, or mode default.

## Consequences

- A later implementation lane can add permission modes without conflating that
  work with privacy disclosure or prompt-shaping policy.
- `src/tools/operator/policy.rs` becomes the single review boundary for later
  operator-policy evaluation work.
- Repo-local permission commands remain useful, but they are no longer treated
  as the whole overlay architecture.
- Configuration, transport, UI, and transcript work must all respect the same
  protected-path and fail-closed invariants.

## Implementation Notes

- Extend ADR-022 by keeping approval-first capability policy and the later
  overlay mode engine separate from `RuntimeCorePolicy`.
- Extend ADR-024 by keeping Gap 13 focused on interactive commands while this
  ADR defines later mode precedence, protected-path policy, and non-interactive
  refusal rules.
- Extend ADR-042 by keeping `ToolPolicy` and alias-based tool approval intact
  while later overlay decisions are evaluated at the operator-policy seam.

## Anchor Tests Required Before Merge of Enforcement Code

- `test_permission_overlay_protected_paths_deny_before_mode_default`
- `test_permission_overlay_deny_rules_override_allow_rules`
- `test_permission_overlay_plan_mode_rejects_mutating_actions`
- `test_permission_overlay_bypass_still_rejects_protected_paths`
- `test_permission_overlay_untrusted_workspace_forces_default_mode`
- `test_permission_overlay_noninteractive_prompt_required_refuses`