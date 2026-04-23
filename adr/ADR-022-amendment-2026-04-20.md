# ADR-022 Amendment (2026-04-20)

**Status:** Amended  
**Amends:** ADR-022, ADR-034

## Amendment

- Phase G (binary distribution pipeline) and Phase H (macOS app wrapper) are gated on ADR-024 PG-03 tap auto-dispatch.
- All phases prior to G/H are accepted and in effect.
- The only remaining external prerequisite is the tap repository.
