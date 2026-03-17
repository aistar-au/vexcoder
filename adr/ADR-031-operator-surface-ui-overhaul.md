# ADR-031: Operator Surface UI Overhaul

- **Status:** Proposed
- **Date:** 2026-03-18
- **Deciders:** Core maintainer
- **Depends on:** ADR-022, ADR-027, ADR-028, ADR-030
- **Supersedes:** None
- **Superseded by:** None

## Context

The operator surface defined by ADR-022 Phase 6 introduced a task-first
four-region layout (header, activity trail, output pane, input pane). ADR-028
established the application facade and transport boundary model. ADR-030
defined the runtime as a task-state-owned orchestrator.

The current implementation has reached the point where the activity pane
reflects in-flight tool calls (ADR-030 invariant 6 fix), the pipeline
activity rows are capped at six lines, and the rendering path uses structured
prefix styling.

The next step is a targeted UI overhaul that:

- extends the canonical runtime/timeline state to support selected-step
  identity and step lifecycle visibility;
- derives UI-facing timeline rows from canonical task state rather than ad hoc
  activity-row assembly;
- moves scroll ownership from transcript-only behavior to timeline/output
  ownership using already-landed derivation and state;
- formalizes the six-line inspector/dropdown interaction behavior;
- removes obsolete fallback behaviors once the new state and rendering path
  are proven.

This work must preserve the runtime ownership model defined by ADR-030: the
operator surface is a consumer of canonical runtime events and task-derived
state only. It must not become the source of truth for execution.

## Decision

Adopt a batched, task-state-first implementation strategy for the operator
surface overhaul. The UI target is a timeline-driven task view where every
visible row is derived from canonical task state, selection identity is
runtime-visible, and scroll ownership moves from the transcript to the
timeline/output pane pair.

### Operator surface target

The target layout retains the four-region structure from ADR-022 Phase 6:

```text
+-------------------------------------+
| Header: task-id | status | backend  |
+-------------------------------------+
| Timeline / Activity (6-line cap)    |
|  [ok] read_file: README.md          |
|  [->] validate: running...          |
|  [?]  apply_patch: src/main.rs      |
+-------------------------------------+
| Output / Inspector Pane             |
| (selected step detail or output)    |
+-------------------------------------+
| Input: [prompt]  [y/n/s]            |
+-------------------------------------+
```

Key changes from the current implementation:

1. Timeline rows are derived from canonical task-state step lifecycle, not
   from completed-tool-invocation history alone.
2. Selected-step identity is a runtime-visible concept: the UI can track which
   timeline row is focused and display corresponding detail in the output pane.
3. Scroll ownership moves to the timeline/output pair: the timeline scrolls
   independently of the output pane, and the output pane shows content
   appropriate to the selected timeline entry.
4. The six-line activity cap becomes a formal inspector/dropdown: the visible
   window into the full timeline, navigable by keyboard.

### Task-state-first rule for this ADR

Any implementation batch that requires one of the following is task-state-first
work and must land before dependent UI batches:

- a new canonical runtime event category
- a new task-state field or invariant
- a new pending/running/completed step lifecycle
- a new managed command-session lifecycle state
- a new approval pause or resume state
- a new execution-completion condition
- a new selected-item identity used across frames

Renderer and layout work may proceed in parallel against those branches, but
the state-first batch is the merge gate.

## Dispatch, dependency, and task-state control

This ADR permits implementation work to be split across multiple remote
branches and developed in parallel. Parallel development, however, does not
change the runtime ownership model.

The authoritative execution model remains task-state-owned orchestration as
defined by ADR-030:

```text
provider event
-> normalize to runtime event
-> update task state
-> orchestrator decides next action
-> UI reflects canonical task-derived state
```

Branch topology is therefore an implementation convenience, not a source of
runtime truth.

### Core rule

A batch MAY be pushed to a remote branch before its prerequisite lands on
main, but it MUST NOT be merged before the prerequisite task-state and
orchestrator contract it depends on is already present on main.

In other words:

- development may be parallel;
- review may be parallel;
- merge order must follow task-state and orchestrator dependencies.

### Why this rule exists

The operator surface defined by this ADR is a consumer of canonical runtime
events and task-derived state. It is not allowed to become the source of truth
for execution.

That means a UI batch cannot introduce merge-time dependence on unlanded
renderer-local assumptions such as:

- temporary event names
- temporary pending-step trackers
- temporary output buffers
- temporary command lifecycle flags
- temporary approval state derived only in the UI layer

If such state is required for the UI, it must first exist in the landed runtime
or task-state contract, or be derived from already-landed canonical runtime
events.

### Batch classes

Implementation batches under this ADR fall into three classes:

**Independent batches**
These modify behavior that is already supported by the landed task-state
contract and may merge immediately if tests pass.

**Stacked dependent batches**
These depend on another branch or pending merge. They may be pushed and
reviewed in parallel, but they are merge-gated by the prerequisite batch.

**State-first prerequisite batches**
These introduce or modify canonical runtime events, task-state fields,
orchestrator transitions, command-session lifecycle state, approval state, or
other execution truth. These batches must land before dependent renderer or
layout batches.

### Merge-gated dependency rule

When Batch B depends on Batch A, and Batch A changes canonical state or
orchestrator behavior, the repository treats Batch B as:

- parallel-dispatchable
- reviewable
- merge-gated

This means Batch B may exist remotely before Batch A lands, but Batch B must be
rebased or otherwise reconciled onto the post-merge main state before it may
merge.

### Main must remain coherent after every merge

Every merged batch must leave main in a coherent state in which:

- task truth still lives in runtime/task state, not in UI-local heuristics;
- canonical runtime events still drive downstream behavior;
- the orchestrator remains the owner of continuation and completion;
- the UI can render the current truth without depending on unmerged branches.

## Implementation sequencing and branch dispatch

This ADR defines the target operator surface and the required runtime/UI
contracts. It does not require all implementation batches to land in a single
merge.

Implementation work under this ADR MAY be dispatched across multiple remote
branches in parallel, provided that dependency order is respected at merge time.

### Parallel dispatch rule

Batches may be developed and pushed to remote branches concurrently when they
satisfy one of the following:

1. they touch disjoint files or behavior and are independently mergeable; or
2. they are intentionally stacked on a prerequisite branch and are not merged
   until that prerequisite has landed on `main`.

### Merge-gated rule

When Batch B depends on Batch A, the repository treats them as:

- **parallel-dispatchable**
- **merge-gated**

This means:

- Batch B may be implemented and pushed to a remote branch before Batch A lands;
- Batch B may target Batch A during review or be maintained as a stacked branch;
- Batch B MUST NOT be merged to `main` before Batch A is merged and the branch
  is rebased or otherwise reconciled with the then-current `main`.

### Normative rule

The normative contract is the runtime/UI behavior described by this ADR, not the
temporary branch topology used to deliver it.

Temporary stacked branches, prerequisite review branches, or parallel remote
implementation branches are allowed so long as:

- the final merge order preserves runtime invariants;
- each merged batch leaves `main` in a coherent, testable state; and
- no batch relies on undocumented behavior from an unmerged branch.

### Preferred batch shape

Work should be split so that prerequisite batches land first in this order:

1. event/timeline data model (Batch A)
2. runtime-to-UI derivation updates (Batch B)
3. full-screen scroll ownership (Batch C)
4. six-line inspector/dropdown behavior (Batch D)
5. final layout cleanup and removal of obsolete fallback behavior (Batch E)

Independent cleanup, tests, and renderer polish may proceed in parallel on
remote branches, but merges must respect the dependency chain above.

## Batch descriptions

**Batch A — Canonical timeline/task-state extension**
Adds the state the new UI needs: selected step identity, runtime-visible step
lifecycle, command-session row identity. This is merge-gating.

**Batch B — Derivation layer**
Maps canonical runtime/task state into UI timeline rows and inspector content.
Can be developed in parallel, but merge waits for A.

**Batch C — Full-screen scroll ownership**
Moves scroll from transcript-only behavior to timeline/output ownership using
already-landed derivation/state. Can be parallel with B if it only consumes A.

**Batch D — Six-line inspector/dropdown behavior**
Presentation and interaction behavior for the selected row.
Parallel-dispatchable, merge-gated by A and whatever derivation it consumes.

**Batch E — Fallback removal / prompt-yield cleanup**
Removes legacy behavior once the new state and rendering path are proven.
Should merge last.

## Compliance note for dispatchers and agents

Dispatchers and coding agents must use this policy:

- split work aggressively for parallel remote development;
- identify which batches modify execution truth versus presentation only;
- land execution-truth batches first;
- keep dependent UI batches stacked until prerequisites land;
- rebase dependent branches onto main before merge;
- do not merge a renderer batch whose correctness depends on unlanded task-state
  behavior.

Do not treat "parallel" as permission to merge dependent UI batches out of
order.

Use this rule instead:

- **build in parallel where possible**
- **merge in dependency order**
- **rebase dependent batches onto `main` after prerequisites land**

Temporary branch structure is allowed.
Temporary execution truth is not.

## Presentation-only vs execution-truth changes

Not every dependency is equal.

If a batch only changes colors, spacing, row truncation, or inspector border/
title text, that is presentation-only and may be independent.

But if a batch changes what counts as a running step, whether command output
belongs to task state or transcript only, when a step moves from pending to
executing to complete, whether a provider stop event can end a task, or whether
approval pause is runtime-owned or UI-owned, that is an execution-truth
dependency and it must land first.

This distinction is what ADR-030 is designed to protect.

## Consequences

### Positive

- gives the operator surface a coherent derivation path from task state
- formalizes the six-line inspector/dropdown as a first-class interaction
  pattern
- enables parallel development without risking merge-order violations
- preserves the runtime ownership model from ADR-030

### Negative

- requires additional task-state surface area for selected-step identity
- adds merge-gating complexity for dependent batches
- batch E (fallback removal) cannot land until the full rendering path is
  proven

## Non-goals

This ADR does not:

- redefine the runtime execution model (ADR-030 remains authoritative)
- change the application facade boundary (ADR-028 remains authoritative)
- introduce transport or server changes
- change the command-session capture model (ADR-027 remains authoritative)

## References

- ADR-022 — free/open coding agent roadmap (Phase 6: TUI rework)
- ADR-027 — full-screen TUI command session capture
- ADR-028 — application facade and transport boundaries
- ADR-030 — runtime task state and orchestrator control flow
