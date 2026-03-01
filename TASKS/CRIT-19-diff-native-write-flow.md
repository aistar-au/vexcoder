# Task CRIT-19: Diff-Native Write Flow

**Target File:** `src/tools/operator.rs`, `src/edit_diff.rs`

**ADR:** ADR-022 Phase 3

**Depends on:** FEAT-17 (`test_command_runner_one_shot_captures_exit_code_and_stdout` must be green)

---

## Issue

`ToolOperator::write_file` and `edit_file` in `src/tools/operator.rs` write directly
to disk with no diff preview and no approval gate. ADR-022 requires that all mutations
to existing files go through a patch-generation step and produce an `ApprovalRequest`
before any write is applied.

---

## Decision

1. Add a `propose_patch(path, old_content, new_content) -> Result<PendingPatch>` method
   to `ToolOperator`. `PendingPatch` carries the unified diff string and the new content.
2. Add `apply_patch(pending: PendingPatch) -> Result<()>` that performs the actual write.
3. Change `write_file` for existing files to call `propose_patch` and return the
   `PendingPatch` instead of writing. Callers must call `apply_patch` explicitly.
4. New-file creation (path does not exist) follows the same gate.
5. The unified diff is generated using the existing `src/edit_diff.rs` utilities.

The approval wiring (surfacing the `PendingPatch` through `ApprovalPolicy`) is done in
CORE-16. This task only introduces the `PendingPatch` type and the two-step write path.

---

## Definition of Done

1. `ToolOperator::write_file` on an existing path returns a `PendingPatch`, not a
   written file.
2. The file on disk is unchanged until `apply_patch` is called.
3. Rejecting (dropping) a `PendingPatch` without calling `apply_patch` leaves the file
   unchanged.
4. Existing read-only tool tests remain green.
5. `cargo test --all-targets` is green.

---

## Anchor Verification

`test_write_existing_file_requires_approval_not_direct_write`

```rust
#[test]
fn test_write_existing_file_requires_approval_not_direct_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("target.rs");
    std::fs::write(&path, "fn old() {}").unwrap();
    let op = ToolOperator::new(dir.path().to_path_buf());
    // propose_patch must not touch disk
    let pending = op.propose_patch(
        path.to_str().unwrap(), "fn old() {}", "fn new() {}"
    ).expect("propose failed");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "fn old() {}");
    // apply_patch must write
    op.apply_patch(pending).expect("apply failed");
    assert!(std::fs::read_to_string(&path).unwrap().contains("fn new()"));
}
```

**What NOT to do:**
- Do not wire approval prompts through the TUI in this task — that is CORE-16.
- Do not modify `src/state/`, `src/api/`, or `src/runtime/`.
- Do not remove path-safety guards from `ToolOperator`.
- Do not change git tool helpers.
