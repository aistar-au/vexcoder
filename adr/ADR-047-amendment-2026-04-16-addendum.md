# ADR-047 Amendment (2026-04-16) Addendum

**Status:** Amended  
**Amends:** ADR-047-amendment-2026-04-16

## Addendum

- `json_handoff.rs` envelope metadata additions (Phase A) target `event_id`, `emitted_at`, `source`, optional `request_id`, `parent_event_id`.
- `ToolCallParser` and text-protocol fallback are listed as retirement targets after envelope schema is extended; not deprecated yet.
- `RuntimeMode` and `FrontendAdapter` are simplification targets, not current code-shape refactoring items.
