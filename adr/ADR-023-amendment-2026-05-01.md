# ADR-023 Amendment (2026-05-01): Non-Consecutive Repeated Read-Only Signature Detection

**Status:** Amended  
**Amends:** ADR-023  
**PR:** #429 (`work/vexcoder-remove-tagged-xml-fallback`)

## Amendment

### Loop guard limitation: consecutive-only comparison

ADR-023 established bounded pulse cycles with loop guards. The original read-only loop guard in `send_message.rs` compared the current round's tool-call signature against only the immediately preceding round:

```rust
if current_signature == previous_round_signature {
    repeated_read_only_rounds += 1;
} else {
    repeated_read_only_rounds = 0;
}
previous_round_signature = Some(current_signature);
```

This failed to detect the restart-at-offset-0 pattern that emerges under context pressure. When a local model with an 8192-token context exhausts its window (~7000+ tokens after 7 or more file reads), it may oscillate through a sequence such as:

```
read_file("generate_release_notes.py", offset=0)    → round N
read_file("generate_release_notes.py", offset=50)   → round N+1
read_file("generate_release_notes.py", offset=100)  → round N+2
read_file("generate_release_notes.py", offset=0)    → round N+3   ← restart
```

Because round N+3 is identical to round N but not round N+2, the consecutive comparison resets `repeated_read_only_rounds` to 0. The guard never fires, and the model loops indefinitely.

### Fix: HashSet-based signature accumulation (commit 2f9f64a)

The outer loop now maintains a `HashSet<Vec<String>>` of all read-only signatures seen in the session:

```rust
let mut seen_read_only_signatures: HashSet<Vec<String>> = HashSet::new();
```

The guard for read-only rounds becomes:

```rust
if is_read_only_tool_round(&tool_use_blocks) {
    if !seen_read_only_signatures.insert(current_signature.clone()) {
        repeated_read_only_rounds += 1;
    } else {
        repeated_read_only_rounds = 0;
    }
} else {
    repeated_read_only_rounds = 0;
}
```

`HashSet::insert` returns `false` when the element was already present, meaning this signature has been seen before at any point in the conversation, not just the previous round. This catches:

- Immediate consecutive repetition (previous behavior preserved).
- Any-distance repetition: if the model returns to a signature it used 5 rounds earlier, the guard increments on the first recurrence.

`tool_round_signature` computes `Vec<String>` of `"name:{input_json}"` for every `ContentBlock::ToolUse` in the round. Two rounds with the same tool calls (name + fully-materialized input JSON) produce the same signature and collide in the set.

`repeated_read_only_rounds` still controls the threshold: the first recurrence triggers the nudge prompt; the second fires the hard guard and terminates the loop. The `previous_round_signature` field is retained for the mutating-round guard path, which uses consecutive comparison and is unaffected by this change.

### Boundary behaviour

- A search round between two identical read-only rounds resets `repeated_read_only_rounds` to 0 (search is not read-only under this guard) but does NOT remove the read-only signature from `seen_read_only_signatures`. Returning to the read-only signature after the search increments the counter again.
- The `seen_read_only_signatures` set grows monotonically for the conversation session. It is reset only when the outer conversation loop restarts (e.g., after a new user message).
