# ADR-031: Operator Surface UI Overhaul

- **Status:** Accepted
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

The current implementation has now converged on a direct ANSI CLI/app surface
where the scrolling transcript owns the full upper body, live tool/approval/
orchestrator updates render as transcript paragraphs, and the only persistent
bottom regions are the multiline composer and separate status bar.

The active operator surface also keeps the composer as a larger multiline
surface so slash commands, `@path` expansion, pasted blocks, and long prompts
remain usable without dropping out of fullscreen task mode, including
visual-row cursor navigation for wrapped prompt text. That composer behaves as
a responsive fullscreen surface: it reflows against the current display row
and column budget so resizing or snapping the display does not leave a stale
fixed-height prompt reservation behind. Short transcripts now render from the
top of the transcript pane rather than hugging the composer edge.

Batch A and Batch B are now merged into `main`, so the remaining ADR-031
scope is the post-derivation alignment pass:

- aligns transcript/output semantics across `src/app/layout.rs`,
  `src/ui/render/`, and the ratatui renderer paths;
- removes obsolete fixed-row assumptions once the adaptive task surface is the
  only current layout path;
- replaces transient formatting/string-coupling where the renderer still
  derives structured header fields from flattened status text.

This work must preserve the runtime ownership model defined by ADR-030: the
operator surface is a consumer of canonical runtime events and task-derived
state only. It must not become the source of truth for execution.

The ADR-030 verification suite completed on 2026-03-25, so ADR-031 now depends
on an accepted runtime control-flow contract rather than an unverified active
draft. ADR-030 is also the semantic correctness guarantee for multi-agent
handoffs (Invariants 1, 4, 5); the operator surface inherits that guarantee by
consuming only task-derived state. The remaining merge gate is therefore
dependency ordering and state consumption, not open control-flow correctness
coverage.

## Decision

Adopt a batched, task-state-first implementation strategy for the operator
surface overhaul. The UI target is a task-derived fullscreen CLI/app view where
every visible paragraph is derived from canonical task state, selection
identity remains runtime-visible, the transcript/composer/status regions scale
with current display rows and columns, and status or composer content stays
human-readable.

### Operator surface target

The accepted direct ANSI surface now uses one top transcript pane, the
persistent composer, and the status bar:

```text
+--------------------------------------------------------+
| Scrolling transcript / task-state paragraphs           |
|  [thinking] Mapping adjacent sectors... 2.5s | read... |
|  [tool] read_file · src/main.rs · State synchronized.  |
|  [approval] apply_patch · awaiting approval            |
+--------------------------------------------------------+
| Composer / Approval card                               |
+--------------------------------------------------------+
| Status bar                                             |
+--------------------------------------------------------+
```

Key changes from the current implementation:

1. The scrolling transcript becomes the authoritative visible stream for
   waiting status, tool activity, approvals, orchestrator updates, and
   assistant output.
2. Structured timeline entries remain derived from canonical task-state step
   lifecycle, but the direct ANSI surface no longer reserves a separate
   activity strip for them.
3. Scroll ownership moves to the task surface: the transcript redraws from the
   same task-derived state, starts at the top of its pane, and scrolls upward
   indefinitely as new paragraphs arrive.
4. Telemetry remains inline in transcript paragraphs, while the separate status
   bar folds compact telemetry, git branch (`\ue0a0branch`), and token counters
   (`↑sent ↓received`) into one truncated line rather than reintroducing a
   dedicated fixed pane.
5. The composer remains a multiline prompt surface with persistent affordances
   for slash commands, `@path` expansion, pasted blocks, and newline insertion,
   and it auto-fits within the current fullscreen viewport as display rows or
   columns change.
6. Enriched tool-call paragraphs show the first 3 evidence lines of output
   followed by a `+N more lines` overflow indicator when output exceeds
   the cap.
7. Cross-platform resize robustness: the draw engine enforces a minimum viable
   surface (10×4), resets all hash state on resize, and performs a full repaint
   to ensure consistent layout across Windows Terminal, GNOME, and macOS.

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

### Amendment — transcript ownership and detail surfaces

The fullscreen operator surface remains transcript-first. The primary
scrolling body owns routine operator-visible content: waiting status,
assistant output, compact tool paragraphs, command-session summaries,
compact approval paragraphs, and short evidence snippets.

Detail work does not permanently occupy the transcript. Long diffs, long tool
evidence, timeline browsing, approval detail, and inspector drill-down use
overlays, pagers, or transient selection modes instead of restoring a fixed
activity or telemetry pane.

Timeline discoverability remains required, but a permanently reserved activity
strip is not restored. The operator may enter and leave detail modes without
changing the transcript-first contract.

Compact status-bar cues and explicit return-to-live navigation keep transient
timeline browsing visible without restoring a permanent activity strip.

The navigator or mapping theme applies to wording, emphasis, and spatial cues
only. It does not change the structural contract of transcript body, composer,
and compact status bar.

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

That means a UI batch cannot introduce merge-time dependence on renderer-local
assumptions that have not been merged into `main`, such as:

- temporary event names
- temporary pending-step trackers
- temporary output buffers
- temporary command lifecycle flags
- temporary approval state derived only in the UI layer

If such state is required for the UI, it must first exist in the runtime or
task-state contract already merged into `main`, or be derived from canonical
runtime events already merged into `main`.

### Batch classes

Implementation batches under this ADR fall into three classes:

**Independent batches**
These modify behavior that is already supported by the task-state contract
already present on `main` and may merge immediately if tests pass.

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
rebased or otherwise updated to the post-merge main state before it may
merge.

### Main must remain coherent after every merge

Every merged batch must leave main in a coherent state in which:

- task truth still remains in runtime/task state, not in UI-local heuristics;
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
  until that prerequisite has been merged into `main`.

### Merge-gated rule

When a dependent batch depends on a prerequisite batch, the repository treats
them as:

- **parallel-dispatchable**
- **merge-gated**

This means:

- the dependent batch may be implemented and pushed to a remote branch before
  the prerequisite lands;
- the dependent batch may target the prerequisite during review or be
  maintained as a stacked branch;
- the dependent batch MUST NOT be merged to `main` before the prerequisite is
  merged and the branch is rebased or otherwise synced to the then-current
  `main`.

### Normative rule

The normative contract is the runtime/UI behavior described by this ADR, not the
temporary branch topology used to implement it.

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

Batches A through E are merged into `main`. Any further implementation lane
must be recorded by ADR amendment before dispatch; the 2026-04-08 amendment
below does exactly that for the terminal-owned history cutover.

## Batch descriptions

**Batch A — Canonical timeline/task-state extension**
Adds the state the new UI needs: selected step identity, runtime-visible step
lifecycle, command-session row identity, and follow-mode ownership for timeline
selection. In the current implementation track this means:

- stable `step_id` values for timeline rows derived from pending/completed tool
  calls;
- explicit row identity for command-session entries;
- runtime-visible `Approved` lifecycle state between operator approval and tool
  completion;
- task-surface follow mode so selection can stay pinned to the latest step
  until the operator scrolls away.

This is merge-gating.

**Batch B — Derivation layer**
Maps canonical runtime/task state into UI timeline rows and inspector content.
This batch is merged into `main`.

Batch B implementation on main includes stable timeline entries, selected step
focus, inspector/transcript routing from canonical task state, and unified
derivation for structured timeline rows so command-session rows remain
visible alongside other in-progress task steps. The legacy `activity_rows`
derivation was removed in Batch E.

**Batch C — Full-screen scroll ownership**
Moves scroll from transcript-only behavior to timeline/output ownership using
derivation/state already merged into `main`. Can be parallel with B if it only
consumes A.

**Batch D — Six-line inspector/dropdown behavior**
Presentation and interaction behavior for the selected row.
Parallel-dispatchable, merge-gated by A and whatever derivation it consumes.

**Batch E — Fallback removal / prompt-yield cleanup** *(merged)*
Removed legacy `activity_rows` derivation (`task_activity_rows_from()`),
fallback rendering paths (`draw_timeline_fallback()`,
`draw_legacy_activity_row()`), and the `legacy_row` field from
`TaskStepView`. The structured timeline renderer is now the sole rendering
path.

## Compliance note for operators and agents

Operators and coding agents must use this policy:

- split work aggressively for parallel remote development;
- identify which batches modify execution truth versus presentation only;
- merge execution-truth batches first;
- keep dependent UI batches stacked until prerequisites merge;
- rebase dependent branches onto main before merge;
- do not merge a renderer batch whose correctness depends on task-state
  behavior that is not yet merged into `main`.

Do not treat "parallel" as permission to merge dependent UI batches out of
order.

Use this rule instead:

- **build in parallel where possible**
- **merge in dependency order**
- **rebase dependent batches onto `main` after prerequisites merge**

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

## Amendment — 2026-04-08: Terminal-owned history and live bottom viewport

### Scroll ownership change

The original operator surface target described the scrolling transcript as an
app-owned upper body where the TUI maintains scroll offsets across the full
committed history. Analysis of six concrete scroll defects in the current
implementation demonstrates that this model does not scale to indefinite
sessions:

1. The idle-mode `Paragraph::new().scroll()` path uses a `u16` offset
   (`src/ui/render/mod.rs`), capping reviewable history at ~65,000 display
   rows.
2. Turn-boundary resets in `src/app/turn.rs` and `src/app/model_update.rs`
   force `transcript_scroll_offset = 0`, destroying the operator's review
   position whenever a turn completes or an error occurs.
3. `expand_rows_for_display()` in `src/ui/render/transcript.rs` performs
   O(n) full-history re-expansion every frame, growing linearly with session
   length.
4. The idle path is always tail-pinned with no interactive scroll support.
5. The six-row inspector cap in `src/app/layout.rs` hard-limits detail
   surface height.
6. Structural no-wrap in `src/ui/render/transcript.rs` and
   `src/ui/render/mod.rs` miscounts display rows for bracket-delimited
   transcript markers.

### Terminal-owned history contract

The operator surface target is amended. The terminal now owns committed
transcript history above the viewport:

The preferred implementation path is ratatui-native. The current tree already
pins `ratatui = 0.29`, which provides `Viewport::Inline(..)`,
`Terminal::with_options(..)`, and `Terminal::insert_before(..)` for an inline
reserved viewport with committed lines inserted above it. Any app-local
`TerminalHistorySink` should therefore be a thin wrapper over the ratatui
terminal API rather than a bespoke escape-sequence subsystem.

This remains compatible with the current frontend bootstrap because
`src/terminal.rs` enables raw mode but does not enter the alternate screen.
The main screen and its scrollback remain available as the owner of committed
history, and Batch F must preserve that property.

1. **Terminal owns committed history.** Stable transcript paragraphs are
   flushed upward through a terminal history sink (`TerminalHistorySink`)
   as soon as they become committed. The host terminal's scrollback buffer
   becomes the indefinite review surface for committed content.

2. **App owns only the live tail.** The application retains ownership of the
   live bottom viewport: the current response tail, composer or approval
   surface, and status line. This reserved viewport occupies the bottom
   portion of the terminal.

3. **Committed paragraphs flush upward.** `flush_committed_history()` writes
   stable paragraphs into terminal history using the preferred insertion
   mode. The app does not maintain scroll offsets for committed content on
   the main surface.

4. **Full-session review uses host scrollback or a transcript overlay.** The
   operator reviews committed history by scrolling the host terminal's
   scrollback buffer. An explicit transcript overlay (detail surface) is
   available for structured navigation within the app, but it is not the
   primary review mechanism.

5. **Compatibility ladder.** The terminal history sink supports three
   insertion modes in priority order:
   - **Scroll-region insertion** (preferred): on the ratatui-native path,
     `ManagedTuiFrontend` switches from `Terminal::new(..)` to
     `Terminal::with_options(.. Viewport::Inline(..))` and flushes committed
     rows with `Terminal::insert_before(..)`. When ratatui's
     `scrolling-regions` feature is enabled, that API uses backend
     scroll-region insertion above the reserved live viewport.
   - **Newline fallback**: when scroll-region insertion is unavailable, the
     same inline-viewport design may fall back to ratatui's
     non-scrolling-region insertion path or explicit newline writes that
     scroll the terminal naturally.
   - **Owned-transcript fallback**: when neither terminal insertion mode is
     viable (e.g., non-terminal output), the existing app-owned transcript
     renderer (`render_messages`, `render_task_layout`) remains active as
     the last fallback.

6. **Current app-owned scroll path is transitional.** The scroll logic in
   `src/ui/render/mod.rs`, `src/ui/render/transcript.rs`,
   `src/app/scroll.rs`, and `src/app/turn.rs` that maintains
   `transcript_scroll_offset` as a main-surface position tracker is now
   transitional. It remains available for the owned-transcript fallback and
   detail overlays but is no longer the target architecture for the primary
   operator surface.

### Proposed state and type names

The following names are documented for implementation reference:

- `TerminalHistorySink` — abstraction for committed transcript insertion
- `TerminalHistoryInsertMode` — enum: `ScrollRegionInsert`,
  `BottomNewlineFallback`, `OwnedTranscriptFallback`
- `LiveBottomViewportState` — state for the reserved live viewport
- `committed_history_flush_cursor` — position tracker for flush progress
- `pending_history_flush_rows` — rows awaiting flush to terminal history
- `live_tail_rows` — rows in the active live viewport
- `detail_overlay_rows` — rows in the detail/overlay surface
- `detail_overlay_scroll_offset` — scroll offset for overlay-only navigation
- `surface_mode` — current `TerminalHistoryInsertMode` selection
- `reserved_viewport_text_width` — wrap budget for the live bottom viewport

### Relationship to existing batches

Batches A through E remain valid as merged. This amendment adds a Batch F
scope: the terminal-owned history cutover. Batch F is merge-gated by the
existing Batches A–E and by ADR-041 D17–D22 (terminal history sink
technical decisions).

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
- batch E (fallback removal) merged after the full rendering path was proven
  end-to-end across Batches A–D

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
