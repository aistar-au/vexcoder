# ADR-031: Operator Surface UI Overhaul

**Status:** Accepted (Batches A–E merged; 2026-04-08 host-scrollback amendment deprecated 2026-04-09)  
**Chain:** ADR-022, ADR-027, ADR-028, ADR-030

## Context

The pre-ADR-031 TUI used a fixed layout with no scrolling transcript and mixed ownership between a host scrollback buffer and the application. The overhaul establishes a single app-owned transcript surface.

## Decision

- Adopt task-state-first fullscreen layout: scrolling transcript (app-owned), persistent composer, status bar.
- Transcript is the authoritative visible stream for status, tool activity, approvals, and orchestrator updates.
- Scroll ownership moves to the task surface; viewport starts at top on session open.
- Compact telemetry: six-line inspector cap; enriched tool paragraphs show 3 evidence lines + overflow indicator.
- Cross-platform resize robustness with 10×4 minimum viable surface.
- Detail views use overlays and pagers, not permanent activity strips.
- Batch development may proceed in parallel but batches merge in dependency order (state-first, then UI).
- No `HostScrollbackSink` or ratatui inline viewport insertion; owned transcript only.

## References

- [`ratatui`](https://docs.rs/ratatui) — TUI framework
- [`crossterm`](https://docs.rs/crossterm) — console backend

## Amendment 2026-05-16 — ratatui 0.30 seam coverage staging

PR #384 localized dependency version ownership in the root manifest, but the
ratatui seam inventory remained narrower than the modules that absorb API churn
in practice. The stable operator surface now depends on the facade
`src/ui/tui.rs`, the layout adapter `src/ui/layout.rs`, and the render adapters
in `src/ui/render/`. A 100-entry ratatui 0.30 audit places current coverage at
17 percent and shows that the highest-value missing APIs are concentrated in
text/style/layout/block builders rather than in stateful widgets or buffer-level
rendering.

This amendment records a maintenance rule rather than a new product
requirement.

- Treat the facade plus the small layout/render adapter set as one
  upgrade-maintained ratatui seam.
- The first maintenance batch prioritizes 10 APIs: `Text::raw`,
  `Text::from_iter`, `Stylize` semantic accent shorthands, `Stylize`
  neutral-tone shorthands, `Stylize` modifier shorthands, `Style::new`,
  `Block::bordered`, `Block::title_top`, `Layout::vertical(...).areas(...)`,
  and `Constraint::Fill`.
- Do not chase raw coverage by wiring APIs that do not match the present
  interaction model. `List`, `Table`, `Scrollbar`, `Tabs`, `Gauge`, custom
  `Widget` implementations, and buffer-level writes remain optional until
  selection state, structured tabulation, or reusable low-level widgets become
  architectural requirements.

Official ratatui 0.30 documentation makes these APIs the idiomatic surface for
new code. Comparable public Rust TUI codebases reviewed during this change
show the same ordering: text/layout/style first; stateful widgets only when
the UI genuinely needs external widget state.

The remaining 90 audited APIs stay staged into four follow-on maintenance
batches: Batch 2 for text/alignment/block/layout refinements, Batch 3 for
stateful widgets only when the interaction model requires them, Batch 4 for
custom widgets and buffer-level rendering APIs, and Batch 5 for niche or
deliberately deferred surfaces.

### References (added)

- RAT-2: ratatui `Text` docs — constructors and iterator conversion (`https://docs.rs/ratatui/latest/ratatui/text/struct.Text.html`).
- RAT-3: ratatui `Style` and `Stylize` docs — `Style::new` and named style shorthands (`https://docs.rs/ratatui/latest/ratatui/style/struct.Style.html`, `https://docs.rs/ratatui/latest/ratatui/style/trait.Stylize.html`).
- RAT-4: ratatui `Block` docs — `Block::bordered` and `title_top` (`https://docs.rs/ratatui/latest/ratatui/widgets/struct.Block.html`).
- RAT-5: ratatui `Layout` docs — `Layout::vertical(...).areas(...)` and `Constraint::Fill` (`https://docs.rs/ratatui/latest/ratatui/layout/struct.Layout.html`, `https://docs.rs/ratatui/latest/ratatui/layout/enum.Constraint.html`).
