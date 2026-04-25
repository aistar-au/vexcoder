# ADR-009: TUI Interaction Contract

**Status:** Accepted  

## Decision

- TUI input events are handled synchronously in the event loop; no deferred processing.
- Model pulses are spawned on a `tokio` task; the event loop remains responsive during streaming.
- `KeyCode::Enter` submits the prompt; `KeyCode::Esc` cancels pending input.

## References

- [`crossterm`](https://docs.rs/crossterm) — key event types
