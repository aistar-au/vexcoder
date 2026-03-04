# Task CORE-16: Capability-Based Approval Policy

**Target File:** new `src/runtime/approval.rs`, `src/runtime.rs`

**ADR:** ADR-022 Phase 4

**Depends on:** CRIT-19 (`test_write_existing_file_requires_approval_not_direct_write` must be green)

---

## Issue

No capability-gating approval system exists. `RuntimeCorePolicy` in
`src/runtime/policy.rs` handles prompt-shaping only (text sanitization, tool-evidence
hints) and must not be repurposed. ADR-022 requires a separate `ApprovalPolicy` that
evaluates each `Capability` against a grant table loaded from `VEX_POLICY_FILE`.

---

## Decision

1. Add `src/runtime/approval.rs` with:

```rust
pub enum PolicyAction { Allow, Prompt(ApprovalScope), Deny }

pub trait ApprovalPolicy {
    fn evaluate(&self, capability: Capability) -> PolicyAction;
    fn load_from_file(path: &std::path::Path) -> Result<Self> where Self: Sized;
}
```

2. Implement `FileApprovalPolicy` that parses `.vex/policy.toml` with values:
   `"allow"`, `"deny"`, `"once"`, `"task"`, `"session"`.
3. Default policy (no file present): `ReadFile` → `Allow`; all mutating and command
   capabilities → `Prompt(ApprovalScope::Once)`.
4. Export from `src/runtime.rs`. Keep entirely separate from `RuntimeCorePolicy`.

---

## Definition of Done

1. `FileApprovalPolicy::load_from_file` parses a valid `.vex/policy.toml`.
2. `ReadFile` evaluates to `PolicyAction::Allow` under the default policy.
3. `ApplyPatch` and `RunCommand` evaluate to `PolicyAction::Prompt(_)` under the
   default policy.
4. `RuntimeCorePolicy` in `src/runtime/policy.rs` is not modified.
5. `cargo test --all-targets` is green.

---

## Anchor Verification

`test_approval_policy_read_file_auto_allows_without_prompt`

```rust
#[test]
fn test_approval_policy_read_file_auto_allows_without_prompt() {
    let policy = FileApprovalPolicy::default();
    assert!(matches!(policy.evaluate(Capability::ReadFile), PolicyAction::Allow));
    assert!(matches!(policy.evaluate(Capability::ApplyPatch), PolicyAction::Prompt(_)));
    assert!(matches!(policy.evaluate(Capability::RunCommand), PolicyAction::Prompt(_)));
    // ApplyPatch grant does not imply RunCommand
    let patch_grant = PolicyAction::Prompt(ApprovalScope::Once);
    let _ = patch_grant; // scope is per-capability, not shared
}
```

**What NOT to do:**
- Do not modify `src/runtime/policy.rs` or the `RuntimeCorePolicy` trait.
- Do not add `ApprovalPolicy` evaluation to the TUI in this task — that is FEAT-19.
- Do not modify `src/tools/`, `src/state/`, or `src/api/`.
- Do not add new `UiUpdate` variants.
