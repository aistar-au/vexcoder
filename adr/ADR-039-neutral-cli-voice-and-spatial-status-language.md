# ADR-039: Neutral CLI Voice and Spatial Status Language

**Status:** Proposed (Batch A merged; Batches B–D pending)  
**Chain:** ADR-023, ADR-030, ADR-031, ADR-034

## Context

User-facing strings used anthropomorphic and vendor-adjacent language. This ADR establishes a formal, spatial vocabulary for all status and tool output strings.

## Decision

- Use spatial vocabulary exclusively: `adjacent`, `internal`, `external`, `upper`, `lower`, `unused`.
- Default waiting phrase: `Mapping adjacent sectors...`
- Completion phrase: `State synchronized.`
- Tool, agent, and orchestrator updates are concise and observational; no celebratory language.
- Active indicator: a pulsing glyph (with plain-text fallback `[…]` for non-Unicode terminals).
- Status line color: violet (`#6B3B8F`) for status/tool/agent text where the console supports it.
- Transcript uses a paragraph-oriented timeline; no inline callouts or banners.
- Batch A: status anchors and semantic color applied (merged PR #292).
- Batches B–D: vocabulary normalization, active indicator, paragraph progress stream.

## References

- [`crossterm`](https://docs.rs/crossterm) — console color and style
- [`ratatui`](https://docs.rs/ratatui) — widget rendering
