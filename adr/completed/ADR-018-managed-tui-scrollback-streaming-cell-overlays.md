# ADR-018: Managed TUI — Scrollback, Streaming Cell, Overlays

**Date:** 2026-02-22
**Status:** Deprecated by ADR-027
**Deciders:** Core maintainer
**Related tasks:** CORE-15, CORE-16, CORE-17, FEAT-17, FEAT-18, FEAT-19
**ADR chain:** ADR-006, ADR-007, ADR-009, ADR-010
**Deprecates:** ADR-017 (on acceptance + migration completion)

## Context

Current runtime accepted path is append-cli (`src/bin/vex.rs`). It is
simple and stable, but it does not provide managed in-app scrollback or a
viewport model that supports transcript navigation while composing input.

For long sessions, users need explicit viewport control (`scroll_offset`,
`auto_follow`) and deterministic streaming behavior in one active render cell.
This aligns with common open-source Rust TUI patterns (`ratatui`,
`crossterm`, `tokio`).

## Decision

1. Move primary runtime interaction to managed TUI with three panes
   (status/transcript/input).
2. Keep transcript state in one widget (`ChatWidget`) with
   `cells + active + scroll_offset + auto_follow`.
3. Use one active streaming cell; commit on `TurnComplete`.
4. Route keyboard/mouse navigation into widget scrolling APIs.
5. Keep overlays lifecycle-managed (enter/leave paired, panic-safe).
6. The primary cli surface must preserve operator access to pre-session
   shell history. Managed TUI rendering therefore targets the primary cli
   session rather than treating the cli as a disposable full-screen
   surface. Operators must be able to inspect shell output that existed before
   `vex` started using ordinary cli scrollback, while the runtime-owned
   transcript begins at the start boundary.
7. Overlay prompts are the accepted operator-input surface for bounded
   mid-task decisions. Approval, confirmation, resume-selection, credential
   retry, and similar handoff prompts must render in-cli without tearing
   down the active task view. Resolving an overlay must preserve transcript
   state, output-pane state, changed-file visibility, and scroll position, then
   return control to the active task.

## `UiUpdate` Alignment (normative)

This ADR uses the existing file and shapes in `src/runtime/update.rs`:

- `StreamBlockStart { index, block }`
- `StreamBlockDelta { index, delta }`
- `StreamBlockComplete { index }`
- `TurnComplete`

No duplicate streaming variants are introduced.

If tool lifecycle requires dedicated events, only additive tool-specific
variants are allowed (e.g., `ToolCallStarted`, `ToolCallCompleted`) and must
not overlap existing `StreamBlock*` streaming semantics.

## CLI Abstraction Compatibility

`CustomTerminal` may use ratatui insertion APIs for inline viewport behavior.
Implementation must be validated against the pinned ratatui version in this
repo (`ratatui = 0.30`) before task dispatch is considered complete.

The managed TUI is not permitted to rely on a rendering strategy that makes
pre-session shell history unreachable until process exit. Primary-cli
rendering, inline insertion, or an equivalent cli mode that leaves shell
scrollback available during runtime are acceptable; cli takeover that
hides prior shell history for the duration of the session is not.

## Migration

1. CORE-15 adds cli abstraction and insertion support.
2. CORE-16 adds chat widget state and stream/event mapping.
3. CORE-17 wires app/frontend to the managed viewport and retires direct
   append rendering path.

Until CORE-17 gates are green, ADR-017 remains operational.

## Supersede Mechanics

On acceptance of ADR-018 and successful CORE-17 cutover:

1. Mark `adr/completed/ADR-017-append-single-session-runtime.md` as
   `Superseded by ADR-018`.
2. Update `adr/ADR-README.md` status row for ADR-017 accordingly.
3. Keep ADR-017 in history (do not delete).

## Consequences

- Resolves managed scrollback/viewport limitations of append-only runtime path.
- Increases UI state complexity and requires strict regression coverage.
- Does not change tool-loop guard policy (that remains ADR-016 scope).

## Compliance Notes for Agents

1. Do not split transcript ownership across multiple modules.
2. Keep runtime-core contract boundaries intact (ADR-006/ADR-007).
3. Do not delete superseded ADRs; mark them superseded.
4. Overlay input blocks ordinary prompt submission until the pending decision is
   resolved or cancelled.
5. Bounded multi-choice overlay prompts must accept a small, explicit option
   set and return the runtime to the interrupted task flow after resolution.

---

## Supersession Note (2026-03-13)

**Status: Corrected by ADR-027**

This document specified the full-screen TUI + captured streaming design.
That design has been **corrected and enhanced** per ADR-027 (full-screen
command-session capture alignment).

**Key corrections:**
- Signal handling fixed (Ctrl+C propagation to subprocess)
- Working directory validation added
- Layout underflow fixed (saturating arithmetic)
- PTY support documented (interactive tools)

**Design retained:**
- Full-screen alternate screen (hosted-agent session pattern)
- Command output capture (agent observability)
- StreamBlock events for transcript

See ADR-027 for the corrected implementation details.

## Scroll Architecture Note (2026-04-05)

The `scroll_offset` and `auto_follow` fields described in this ADR have been
removed from `HistoryState`. Follow-mode is now a computed property
(`transcript_scroll_offset == 0`) with no stored boolean. The `ScrollTarget::History`
variant and all legacy scroll methods (`max_scroll_offset`, `set_scroll_to_bottom`,
`clamp_scroll_offset`, etc.) have been deleted. All interactive scrolling runs
through the task-surface draw path via `transcript_scroll_offset` (bottom-anchored)
and `inspector_scroll_offset` (top-anchored). The idle-mode render pins to the
tail with no scroll parameter. All SSE events unconditionally route through the
structured `StreamBlock` pipeline with no alternative rendering routes.

## Architecture Boundary Note (2026-03-15)

ADR-028 defines the longer-term application and transport split that ADR-018 did not make explicit. This superseded ADR must not be read as permission for the long-term application layer to keep mixing TUI/session state, runtime coordination, shared command semantics, or startup wiring. Those concerns now belong behind an explicit application facade, with cli and transport concerns implemented in separate outer modules.
