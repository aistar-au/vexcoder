# ADR-011: TUI Render Loop and Lifecycle

**Status:** Accepted  

## Decision

- Render loop runs at 30 fps max; skips frames if no state change since last render.
- Raw mode entered on TUI start; restored unconditionally on drop via `Drop` impl.
- Panic hook installs a cleanup that restores the console before printing the panic message.

## References

- [`ratatui`](https://docs.rs/ratatui) — render loop
- [`crossterm`](https://docs.rs/crossterm) — raw mode
