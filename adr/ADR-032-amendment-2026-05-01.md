# ADR-032 Amendment (2026-05-01): Read-File Continuation Guidance on Empty-Input Tool Calls

**Status:** Amended  
**Amends:** ADR-032  
**PR:** #429 (`work/vexcoder-remove-tagged-xml-fallback`)

## Amendment

### Root cause: empty-input read_file calls under context pressure

ADR-032 introduced a context-proportional auto-cap for `read_file` that allocates ~10% of the context budget per file read. When a local model operates near its context ceiling (e.g., 8192 tokens with ~7000 tokens consumed after 7+ file reads), the model may generate a `tool_use` block for `read_file` without emitting any `input_json_delta` chunks. The resulting `ContentBlock::ToolUse` has `input: {}` — an empty JSON object.

The existing `missing_read_only_location_prompt` guard catches this case and returns an error to the model:

> "I need an explicit file path. Please call read_file with a 'path' argument."

However, this message provided no information about which file the model was in the middle of reading. The model received the error but had no basis for which path to specify. The result was that the model restarted from offset 0 of the last file it was reading rather than continuing where it left off — precisely the pattern that the ADR-023 loop guard is designed to catch.

### Fix: last_read_file_path tracking (commit 7cd11bd)

The outer conversation loop now maintains:

```rust
let mut last_read_file_path: Option<String> = None;
```

This variable is updated after every successful `read_file` execution:

- **Serial execution path** (single tool call per round): updated immediately after `result.is_ok()` and the undo checkpoint push, before status is set to `Complete`.
- **Parallel execution path** (`completed_calls` loop): updated after `set_tool_call_status` for each completed `read_file` call that returned `Ok`.

When `missing_read_only_location_prompt` fires for a `read_file` call with empty input, the error message is enriched:

```rust
if name == "read_file" {
    if let Some(ref last_path) = last_read_file_path {
        clarification.push_str(&format!(
            " You were most recently reading '{last_path}' — specify that path to continue."
        ));
    }
}
```

The enriched message gives the model a concrete continuation target:

> "I need an explicit file path. Please call read_file with a 'path' argument. You were most recently reading 'scripts/generate_release_notes.py' — specify that path to continue."

This eliminates the root cause of the restart-at-0 pattern: the model receives the path it needs and can re-issue the call with the correct `path` and `offset` arguments.

### Relationship to the loop guard (ADR-023-amendment-2026-05-01)

The ADR-023 HashSet loop guard detects the restart pattern and halts the loop. The `last_read_file_path` fix addresses the root cause: the model no longer has a reason to restart because it receives enough information to continue. Both fixes are complementary:

- The continuation guidance prevents unnecessary restarts in the normal case.
- The loop guard still terminates the session if the model ignores the guidance and repeats a seen signature.
