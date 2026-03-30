# ADR-039: Neutral CLI Voice and Spatial Status Language

- **Status:** Proposed
- **Date:** 2026-03-31
- **Deciders:** Core maintainer
- **Depends on:** ADR-023, ADR-030, ADR-031, ADR-034
- **Supersedes:** None
- **Superseded by:** None

## Context

`vexcoder` already enforces neutral repository language in source, prompts,
and documentation, but the operator-facing CLI surface still lacks a single
voice contract.

Current issues are small in isolation and noisy in aggregate:

1. Progress copy, completion copy, and long-running orchestration updates use a
   mix of generic helper phrasing rather than one display contract.
2. The runtime has canonical machine states such as `completed`, `failed`, and
   `cancelled`, but those protocol values are not the same problem as the
   human-facing transcript copy shown during long-running tasks.
3. ANSI color already carries operational meaning for diffs, yet tool progress,
   orchestrator updates, and ordinary transcript text do not share a documented
   semantic palette.
4. Multi-agent and long-horizon refactor sessions can run for hours, so the
   CLI transcript surface needs low-fatigue status language and a stable visual
   hierarchy rather than celebratory or chatty copy.

The requested direction is still neutral and repository-focused: spatial terms
such as `adjacent`, `internal`, `external`, `upper`, and `lower`; a mapping /
alignment metaphor for in-progress work; a status-based completion phrase; and
clear ANSI role separation that does not disturb established red / green diff
semantics.

The rollout also needs to stay low-gain at first. Early phases should make the
CLI feel more precise without reading like a personality transplant: the first
visible change is the status contract itself (`Mapping adjacent sectors...`,
`State synchronized.`, and the semantic status-color lane), then the broader
vocabulary sweep follows, and only after that does the transcript layout move
toward the denser paragraph stream.

## Decision

Adopt a neutral spatial voice for operator-facing CLI text.

### Scope boundaries

1. This ADR applies to human-facing transcript copy, status text, progress
   indicators, and CLI wording.
2. This ADR does **not** rename canonical machine states, persisted task-state
   fields, JSON API payloads, or protocol values such as `completed`.
3. This ADR does **not** change standard diff semantics: insertions remain
   green and deletions remain red.
4. This ADR does **not** add mascots, conversational persona text, or branded
   product comparisons to the runtime.

### Voice contract

5. Human-facing status text uses neutral spatial and state-based language.
6. Prefer `adjacent`, `internal`, `external`, `upper`, `lower`, and `unused`
   over family, life-cycle, or celebratory metaphors.
7. In-progress mapping work uses `Mapping adjacent sectors...` as the default
   display phrase when a more specific operator-facing status is not available.
8. Human-facing completion text for task / todo surfaces uses
   `State synchronized.`
9. Tool, agent, and orchestrator updates stay concise, paragraph-friendly, and
   observational rather than chatty.

### ANSI semantic roles

10. Default transcript and code text remain phosphor white.
11. Insertions remain green and deletions remain red.
12. Tool-call, orchestrator, and agent-enrichment status text uses deep nebula
    violet or the nearest supported fallback in reduced ANSI environments.
13. If the active renderer supports animated status affordances, the preferred
    active indicator is a single pulsing star glyph paired with the mapping
    status text. Plain-text and accessibility fallbacks may render the text
    without animation.

### Transcript model

14. Long-running work renders as one continuous paragraph-oriented timeline,
    not a sequence of congratulatory callouts.
15. When available, live progress counters such as files processed or agents
    active appear in the same status lane as orchestrator updates.
16. Tool operator output and agent-enrichment output must remain visually
    subordinate to primary code / diff text so the transcript stays legible
    during multi-hour sessions.

### Rollout guardrails

17. Implementation order is intentionally subtle: the first visible rollout
      step is the status contract and semantic color feedback, not a broad copy
      rewrite or transcript-layout change.
18. Batch A should introduce `Mapping adjacent sectors...`,
      `State synchronized.`, and the deep-nebula-violet status lane on existing
      surfaces so operators first encounter the new voice through stable status
      anchors.
19. Batch B should extend the same contract into the wider spatial vocabulary
      set without changing machine-state fields, JSON payloads, or persisted
      schema names.
20. Batch C may add the pulsing-star affordance after the Batch A/B status and
      wording contract is stable in tests and operator-facing docs.
21. Batch D is the first batch allowed to consolidate the long-running
      transcript into the paragraph-oriented progress stream, because it depends
      on the earlier wording and color contracts already being recognizable.

## Planned batches

### Batch A -- Status anchors and semantic color feedback (subtle introduction phase 1)

- Introduce `Mapping adjacent sectors...` as the default human-facing thinking
   text when a more specific operator status is not available.
- Introduce `State synchronized.` on human-facing completion surfaces.
- Move tool-call, orchestrator, and agent-enrichment status text into the
   deep-nebula-violet semantic lane while preserving phosphor-white transcript
   text and green / red diff semantics.
- Keep these changes within the current layout so the first rollout still
   reads as a status refinement rather than a transcript-model change.

### Batch B -- Vocabulary normalization (subtle introduction phase 2)

- Normalize operator-facing copy to the spatial vocabulary set.
- Replace non-neutral relationship wording in transcript text where the
   wording is purely display copy.
- Prioritize the most common low-noise surfaces first: relationship words in
   logs, prompts, and status blurbs that already exist today.
- Do not add transcript layout changes in this batch.
- Leave code symbols, persisted schema, and JSON field names unchanged.

### Batch C -- Active indicator affordance (subtle introduction phase 3)

- Add the pulsing-star active indicator where the renderer supports it.
- Ensure reduced-color and plain-text fallbacks remain readable.

### Batch D -- Paragraph progress stream

- Consolidate long-running tool and agent updates into one paragraph-oriented
   status stream.
- Add live counters for files processed and active agents where the runtime
   already knows those values.
- Render the orchestrator lane as a continuous enriched paragraph while
   keeping code and diff text visually dominant in phosphor white / green / red.
- Keep the code / diff surface visually dominant over status text.

## Consequences

### Positive

- Operator-facing copy gains one consistent voice contract without changing
  machine-facing schemas.
- Long-running sessions become easier to monitor because progress text and ANSI
  roles have a documented hierarchy.
- The runtime keeps its neutral engineering tone while still presenting a more
  deliberate CLI identity.

### Negative

- Some current transcript wording will need churn even where behavior does not
  change.
- Renderer and transcript tests will need updates because copy and status
  labels become part of the contract.
- The paragraph-stream model must be applied carefully so important status
  transitions do not become visually buried.

## Implementation status

Proposed only. No operator-facing runtime strings are changed by this ADR.
The first implementation step is the low-gain Batch A status pass: semantic
color feedback plus `Mapping adjacent sectors...` and `State synchronized.` on
existing surfaces. The pulsing-star affordance and paragraph-stream changes are
intentionally deferred until later phases.

Candidate implementation areas:

- `src/ui/draw/`
- `src/app/`
- `src/state/conversation/`
- `src/runtime/task_state/`

## References

- [ADR-023](https://github.com/aistar-au/vexcoder/blob/main/adr/ADR-023-deterministic-edit-loop.md) — prompt and operator-surface command contract
- [ADR-030](https://github.com/aistar-au/vexcoder/blob/main/adr/ADR-030-runtime-task-state-and-orchestrator-control-flow.md) — canonical runtime state and task lifecycle
- [ADR-031](https://github.com/aistar-au/vexcoder/blob/main/adr/ADR-031-operator-surface-ui-overhaul.md) — operator rendering surface and timeline behavior
- [ADR-034](https://github.com/aistar-au/vexcoder/blob/main/adr/ADR-034-multi-agent-parallel-task-execution.md) — multi-agent progress and watch surfaces