# PR 400 Ratatui/Crossterm TTY Contract Plan

## Goal

Specify and verify the display-control contract for the interactive CLI so the
inline-viewport path remains correct across xterm-class hosts, Windows VT
hosts, and non-TTY fallbacks. This lane addresses compliance and correctness;
it does not propose a widget redesign or renderer rewrite.

## Reference notes

- [x] ECMA-48 is the baseline display-control standard, but it explicitly
  anticipates limited conformance rather than every device implementing every
  control function. References:
  <https://www.ecma-international.org/publications-and-standards/standards/ecma-48/>
  and <https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Standards>.
- [x] ratatui 0.30 documents `TerminalOptions { viewport }` and
  `Viewport::Inline(u16)` for inline rendering below the cursor. References:
  <https://docs.rs/ratatui/0.30.0/ratatui/struct.TerminalOptions.html> and
  <https://docs.rs/ratatui/0.30.0/ratatui/enum.Viewport.html>.
- [x] ratatui 0.30 exposes a `scrolling-regions` feature; this repository
  already enables that feature together with `crossterm_0_29` in the workspace
  dependency seam. Reference:
  <https://docs.rs/crate/ratatui/0.30.0/features>.
- [x] crossterm 0.29 documents `EnableBracketedPaste` /
  `DisableBracketedPaste`, and its crate reference material documents
  alternate-screen and raw-mode behavior. References:
  <https://docs.rs/crossterm/0.29.0/crossterm/event/struct.EnableBracketedPaste.html>
  <https://docs.rs/crossterm/0.29.0/crossterm/event/struct.DisableBracketedPaste.html>
  and <https://docs.rs/crossterm/0.29.0/crossterm/>.
- [x] Windows Console VT behavior requires
  `ENABLE_VIRTUAL_TERMINAL_PROCESSING` for output sequences and
  `ENABLE_VIRTUAL_TERMINAL_INPUT` for VT-style input sequences. Scrolling
  margins and alternate-screen state are per-buffer. Reference:
  <https://learn.microsoft.com/en-us/windows/console/>.
- [x] xterm documents the private sequences this stack effectively relies on:
  scrolling margins (`CSI <t> ; <b> r` / DECSTBM), alternate screen
  (`CSI ? 1049 h` / `CSI ? 1049 l`), and bracketed paste
  (`CSI ? 2004 h` / `CSI ? 2004 l`). Reference:
  <https://invisible-island.net/xterm/ctlseqs/ctlseqs.html>.

## Current repository facts

- `src/ui/tui.rs` is the local integration seam for ratatui and crossterm API
  churn.
- `src/tui_handle.rs` initializes the TTY path with
  `try_init_with_options(TerminalOptions { viewport: Viewport::Inline(..) })`,
  owns bracketed-paste enable/disable, and intentionally stays on the primary
  screen instead of entering `?1049` alternate-screen mode.
- `src/tui_frontend.rs`, `src/ui/render/mod.rs`,
  `src/ui/render/transcript.rs`, and `src/app/scroll.rs` still own most
  viewport sizing, wrapped-row expansion, and review-scroll math.
- The workspace already enables ratatui `scrolling-regions` and the unstable
  rendered-line / widget-ref / backend-writer features in `Cargo.toml`.

## Checklist

- [ ] Batch A: add one explicit display-control note that distinguishes the
  ECMA-48 baseline from xterm- and Windows-specific behavior. Update active
  docs only where the implementation contract is real today.
- [ ] Batch B: harden bootstrap and restore in `src/tui_handle.rs` so
  bracketed paste, cursor cleanup, and line clearing are paired on normal exit,
  panic, and init failure without mutating the non-TTY path.
- [ ] Batch C: add a small capability seam around interactive TTY versus
  VT-capable interactive hosts so Windows VT requirements are documented and,
  where possible, tested instead of remaining implicit.
- [ ] Batch D: re-audit resize and scroll-region correctness across
  `src/tui_frontend.rs`, `src/ui/render/mod.rs`,
  `src/ui/render/transcript.rs`, and `src/app/scroll.rs`; preserve the current
  primary-screen / host-scrollback contract unless a later ADR explicitly
  changes that lifecycle.
- [ ] Batch E: limit input-side claims to behavior the stack actually supports
  today: bracketed paste, resize, cursor visibility, and standard crossterm
  events. Treat modified-key protocols and other xterm-private input encodings
  as out of scope unless they are implemented deliberately.
- [ ] Batch F: add regression coverage in `src/ui/render/tests.rs`,
  `src/app/tests/task_layout.rs`, `src/app/tests/transcript.rs`, and new
  `src/tui_handle.rs` lifecycle tests for fallback and restore behavior.

## Acceptance gates

- [ ] Non-TTY execution remains on the plain fallback path.
- [ ] Primary-screen inline viewport behavior remains intentional and
  documented; there is no accidental alternate-screen regression.
- [ ] Bracketed paste is always disabled on restore, including panic and
  partial-init paths.
- [ ] Resize and scroll math keep transcript ownership stable when host width
  or height changes.
- [ ] Windows VT behavior is either explicitly supported through a capability
  seam or explicitly documented as degraded, rather than remaining implicit.
- [ ] The ratatui / crossterm dependency seam remains localized to
  `src/ui/tui.rs` and `src/tui_handle.rs`.
