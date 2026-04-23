# ADR-035: Undo Checkpoints and Binary-Safe Rollback

**Status:** Accepted  
**Chain:** ADR-024, ADR-030, ADR-031

## Context

No runtime mechanism existed to reverse individual mutating tool calls within a session, forcing users to rely on external version control for accidental overwrites.

## Decision

- `/undo` is a slash command, not a model tool; it operates on the session undo stack.
- Before each mutating tool call, the runtime captures a checkpoint: `(path, Option<Vec<u8>>)`.
- If the file did not exist, checkpoint records `None`; undo removes the newly created file.
- Restore uses `std::fs::write` with the captured bytes, preserving binary content exactly.
- Checkpoints held in-memory only; cleared at session end.
- Scope: session-local rollback; no git operations or patch reversal.

## References

- Rust std [`std::fs::write`](https://doc.rust-lang.org/std/fs/fn.write.html)
- [`Vec<u8>`](https://doc.rust-lang.org/std/vec/struct.Vec.html) — binary-safe payload
