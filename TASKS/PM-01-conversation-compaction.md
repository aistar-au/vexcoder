# Task PM-01: Conversation Compaction

**Target Files:** `src/state/conversation/history.rs`, `src/state/conversation/core.rs`, `src/state/conversation/tests/history.rs`, `src/runtime/task_state/mod.rs`, `src/runtime/task_state/persist.rs`, `src/app/model_update.rs`, `src/config.rs`, `src/config/load/mod.rs`, `src/api/client/mod.rs`, `src/app/tests/memory.rs`, `src/app/tests/session.rs`, `src/batch_mode.rs`, `tests/integration_test.rs`, `tests/live_server_test.rs`

**Depends on:** None (green on current main)

---

## Issue

Long conversations accumulate a growing message history that is sent in full
with every LLM request. This causes:

1. Token usage to grow quadratically with conversation length.
2. Context window overflow for extended sessions, forcing the user to start a
   new conversation.
3. Increased latency per turn as payload size increases.

An overflow fallback already exists via
`ConversationManager::compact_for_context_overflow`, and compaction events are
already recorded in task-state metadata. The current behavior is still
hard-coded: it only triggers after a server-side overflow, it drops older
messages without a generated summary, and it offers no operator-facing
threshold or retention controls.

---

## Decision

### Strategy

Extend the existing overflow compaction path with a configurable proactive
compaction pipeline. The pipeline:

1. Counts tokens in the current message history (approximate, using a
   tokenizer-agnostic byte heuristic or tiktoken where available).
2. When the count exceeds `compaction_threshold` (default: 80% of the model's
   context window), triggers compaction before the server rejects the request.
3. Keeps `compact_for_context_overflow` as the last-resort fallback if the
   proactive path is disabled or misses the limit.
4. Replaces older messages with a compact summary when summary generation is
   available; otherwise it falls back to the current keep-recent pruning
   behavior.
5. Retains the most recent N turns verbatim (configurable via
   `compaction_keep_recent`, default: 4 turns).

### Configuration surface

```toml
# ~/.config/vex/config.toml or .vex/config.toml

[compaction]
enabled              = true       # default: false
threshold_percent    = 80         # trigger at 80% of context window
keep_recent_turns    = 4          # always keep last 4 turns verbatim
summary_max_tokens   = 1024       # max tokens for the summary message
```

### Execution contract

- Compaction runs between turns, never mid-tool-call.
- Summary material replaces the compacted prefix while preserving the
   MessagesV1 invariant that history still begins with a plain user message.
- Original messages are not deleted from the on-disk session log (only from
  the in-memory history sent to the LLM).
- If compaction itself fails (LLM error), log a warning and continue with the
  full history. Never abort a session due to compaction failure.
- Record each compaction event in task-state `context_compaction` metadata.

---

## Constraints

- Do not modify the on-disk session transcript. Compaction is in-memory only.
- Do not change the tool dispatch path. Compaction is purely a message-history
  concern.
- Do not remove the existing `compact_for_context_overflow` fallback.
- The summarization prompt must be hardcoded (not user-configurable) to avoid
  prompt injection via config.
- Token counting must work without an external tokenizer binary. Use a
  byte-based heuristic (4 bytes per token) as the default estimator.
- Preserve the protocol requirement that history begins with a plain user
   message after compaction.
- Must not regress existing tests.

---

## Definition of Done

1. `[compaction]` config section parses without error in user and repo-local
   config layers.
2. When `enabled = true` and token count exceeds threshold, older messages are
   replaced with a summary.
3. Most recent `keep_recent_turns` turns are preserved verbatim.
4. Summary is generated via a dedicated LLM call with a fixed prompt.
5. If summarization fails, conversation continues with full history.
6. Existing overflow fallback remains available as a last resort.
7. On-disk session log is never modified by compaction.
8. `cargo test --all-targets` is green.

---

## Anchor Tests

`test_compaction_triggers_at_threshold`
`test_compaction_preserves_recent_turns`
`test_compaction_summary_replaces_prefix`
`test_compaction_failure_falls_back_to_full_history`
`test_compaction_disabled_by_default`
`test_compaction_config_loads_from_user_layer`

Primary verification anchor:

```rust
#[test]
fn test_compaction_disabled_by_default() {
    // Given a Config with no [compaction] section,
    // compaction.enabled must be false.
}
```
