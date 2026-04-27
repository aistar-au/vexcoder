# ADR-049: Shared Prefix Prompt Caching and Fork Controls

**Status:** Proposed  
**Chain:** ADR-030, ADR-032, ADR-033, ADR-045

## Context

The current TUI turn loop can assemble substantial repository context, but until this change it treated that context as part of one opaque user message. That representation was functionally sufficient for model input, yet it concealed a critical architectural distinction: most repository context is stable across adjacent turns, while git rollups and the immediate user instruction vary from request to request. The codebase already contained two unconnected primitives for handling that distinction: `ApiMessage.cache_hint` on the transport side and `runtime/multiplex_prefix.rs` on the runtime side. Neither was connected to the turn-start path.

Official prompt-caching specifications reviewed for this change converge on the same cache behavior: the reusable prefix spans tool schemas, system instructions, and early message blocks; exact matching is required; and static reusable content should be placed at the beginning of the request. The corresponding response metadata surfaces cached-prefix usage, which makes prefix stability observable in routine operation. Across providers, the stable design principle is not vendor-specific syntax but a reusable prefix boundary whose content remains identical while later suffix content changes.

The operator surface had a parallel discoverability problem. `/fork` already existed and behaved correctly, but the ratatui-native stack exposed no visible affordance for it. The WAI-ARIA button pattern treats buttons as the standard action-triggering control and notes that when a button is activated with a shortcut key, focus commonly remains in the current interaction context. That guidance maps well to the current native frontend, which is keyboard-first and does not rely on pointer capture for routine operation.

## Decision

- The reusable prompt boundary is the shared workspace prefix rendered by `ContextAssembler::render_shared_prefix`, not the full rendered context.
- Mutable git status and recent diff sections remain outside the shared-prefix boundary.
- Turn-start code carries two artifacts when assembled context is available:
  - the full rendered prompt sent to the model;
  - the shared-prefix rendering used to derive cache metadata.
- `ApiClient` computes a deterministic shared-prefix fingerprint from three inputs:
  - the effective system prompt after supplementary prompt, project instructions, and notes are applied;
  - the active structured tool schema set selected by tool policy;
  - the shared workspace context string.
- `ConversationManager` writes that fingerprint into `ApiMessage.cache_hint` on the initiating user turn when shared-prefix context is present.
- The ratatui task composer renders a visible `Fork` action chip and binds `Alt+F` to the existing `/fork` command path.
- Shortcut activation leaves the composer interaction model intact rather than introducing a second focus-management path.
- Provider-specific explicit cache controls may be added later, but they must derive from this runtime-owned shared-prefix contract rather than define a second competing cache boundary.

## Rationale

A runtime-owned shared prefix is the most defensible design because it aligns repository semantics with provider cache behavior without coupling the architecture to one transport.

A fully flat prompt string gives the runtime no stable identity for the reusable portion of a turn. The result is avoidable cache invalidation, weaker observability, and no principled way to compare provider behavior when the stable and varying parts of the prompt are interleaved.

A transport-only design that emits provider cache markers without first naming the reusable runtime prefix would duplicate prompt-boundary logic per backend. The reviewed cache APIs differ in surface syntax, but they reward the same structural property: identical, reused prefix content placed before the request-specific suffix. The runtime should therefore own the shared-prefix boundary once and let transports map it to provider-specific controls later.

The same reasoning applies to the fork affordance. Slash commands remain necessary, but slash-only discovery is recall-based rather than recognition-based. A pointer-driven native control would require a broader key-dispatch channel than the current keyboard-first frontend provides. A visible action chip with a direct shortcut provides the recognisable action surface of a button while preserving the existing interaction model.

## Consequences

- Shared-prefix cache hints now change when the effective system prompt, tool schema set, or stable workspace context changes.
- Routine git churn no longer invalidates the shared-prefix identity because git status and recent diff content are excluded.
- The runtime now has one deterministic cache identity that can be reused across provider integrations.
- The native TUI exposes forking without creating a parallel implementation path; the visible control delegates to the existing `/fork` behavior.
- Per-task persistence of shared-prefix state and subtask inheritance remain future work. This ADR establishes the boundary and fingerprint contract for current turns and the first native fork affordance.

## Alternatives Considered

- Keep a single flat rendered prompt and do not emit cache metadata.
  - Rejected because it leaves `cache_hint` inert and gives the runtime no explicit reusable-prefix model.
- Emit provider-specific cache controls directly from transport code.
  - Rejected because it duplicates boundary logic and makes backend comparison harder.
- Include git status and recent diff in the shared prefix.
  - Rejected because those sections are intentionally volatile and would invalidate the cache during ordinary working-tree activity.
- Expose forking only as `/fork`.
  - Rejected because an important session-control action should not depend entirely on command recall.
- Add a pointer-only fork control in the native interface.
  - Rejected for this slice because the current frontend is keyboard-first and the existing interaction model already supports shortcut-driven action activation.

## References

- [Crossterm Event Module](https://docs.rs/crossterm/latest/crossterm/event/index.html)

