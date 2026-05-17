# ADR-011: TUI Render Loop and Lifecycle

**Status:** Accepted  

## Decision

- Render loop runs at 30 fps max; skips frames if no state change since last render.
- Raw mode entered on TUI start; restored unconditionally on drop via `Drop` impl.
- Panic hook installs a cleanup that restores the console before printing the panic message.

## References

- [`ratatui::try_init_with_options(TerminalOptions { viewport: Viewport::Inline(rows) })`](https://docs.rs/ratatui/0.30.0/ratatui/fn.try_init_with_options.html) - inline-viewport init (`src/tui_handle.rs`)
- [`ratatui::try_restore()`](https://docs.rs/ratatui/0.30.0/ratatui/fn.try_restore.html) - paired restore; installed in panic hook and normal exit (`src/tui_handle.rs`)
- [`frame.area()`](https://docs.rs/ratatui/0.30.0/ratatui/struct.Frame.html#method.area) - replaces deprecated `frame.size()`; used in all render functions
- [`frame.render_widget(widget, area)`](https://docs.rs/ratatui/0.30.0/ratatui/struct.Frame.html#method.render_widget) - primary render call; 14+ call sites across `src/ui/render/`
- [`insert_before(height, fn)`](https://docs.rs/ratatui/0.30.0/ratatui/?search=insert_before) - streaming insert above inline viewport; wrapped in `insert_before_lines()` (`src/tui_handle.rs:45`)
- [`crossterm 0.29`](https://docs.rs/crossterm/0.29.0/crossterm/) - raw mode, bracketed paste, event stream
