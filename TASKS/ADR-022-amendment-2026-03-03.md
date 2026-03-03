# ADR-022 Amendment — 2026-03-03

**Amendment status:** Proposed  
**Amends:** ADR-022 Decision item 1 and the final Compliance note; adds Decision item 11  
**Reason:** ADR-022 as written frames terminal-agent-first and no-editor-integration as permanent identity statements. This amendment re-scopes them as first-milestone sequencing constraints, preserving architectural priority without permanently prohibiting native application packaging or future editor surfaces.

---

## What changes

### Decision item 1 — amended

**Before:**
> `vexcoder` remains terminal-agent-first, not editor-first.

**After:**
> `vexcoder` is terminal-agent-first for the first milestone. The terminal runtime is the canonical execution surface and must remain so at every packaging layer. Native application packaging (e.g. a macOS wrapper) and editor-surface integration (e.g. a VS Code extension) are not in scope for the first milestone and must not be allowed to drive architectural changes to the runtime core.

### Decision item 11 — added

> Native application packaging and editor-surface integration are reserved for post-first-milestone work. When introduced, they must be implemented as thin delivery or UI layers over the unmodified terminal-agent runtime, not as replacements for it. The distinction between a *packaging layer* (wraps the binary, adds OS-native chrome) and an *editor-first application* (replaces the runtime with a language server or extension host) must be preserved in any future ADR that addresses these surfaces.

### Final Compliance note — amended

**Before:**
> Do not convert the product into an editor-first application under this ADR.

**After:**
> Do not introduce native application packaging or editor-surface integration in first-milestone work. Any future milestone that introduces these surfaces must do so via a dedicated ADR that explicitly preserves the terminal-agent core as the canonical runtime.

---

## Rationale

The original wording was written to prevent scope creep during the first milestone, which was the correct intent. However, permanently prohibiting native packaging and editor surfaces would make `vexcoder` harder to distribute and adopt — both of which are required for it to function as a viable, self-hostable coding agent whose dependency chain carries no per-call licensing fee or royalty obligation.

This amendment preserves the sequencing intent (terminal core first, packaging layers second) while leaving room for:

1. **A macOS application wrapper.** A wrapper that launches and manages the `vex` binary, provides OS-native credential storage, and presents a terminal surface in an application window is a *distribution and packaging layer*. The Rust runtime runs unchanged inside it.

2. **A future editor surface.** An extension that shells out to `vex exec` and renders JSONL output in a panel is a *thin editor surface* over an unmodified terminal runtime, provided the extension never owns the agent loop itself.

The critical architectural constraint preserved by this amendment:

> **The terminal-agent runtime must remain the canonical implementation at all packaging layers. If a packaging layer requires changes to `src/runtime/`, `src/api/`, or `src/state/` to function, it is not a packaging layer — it is an architectural fork, and must be treated as such.**

---

## What does not change

- The first milestone scope is unchanged. No packaging or editor work in milestone 1.
- `vex exec` headless mode (ADR-024 Gap 2 / `BatchMode`) is the designated integration point for any future editor surface. It must be stable before an editor extension is designed.
- `RuntimeCorePolicy` and `ApprovalPolicy` remain separate concerns.
- All other ADR-022 Decision items (2–10) are unchanged.
- All other ADR-022 Compliance notes are unchanged.

---

## Application instructions

Apply by editing `TASKS/ADR-022-free-open-coding-agent-roadmap.md` in place:

1. Replace Decision item 1 with the amended text above.
2. Append Decision item 11 after item 10.
3. Replace the final Compliance note with the amended text above.
4. Add a dated amendment header immediately after the `**ADR chain:**` line:

```
**Amendment:** 2026-03-03 — Decision item 1 and the final Compliance note
scoped to first milestone; Decision item 11 added to reserve native packaging
and editor surfaces for post-first-milestone ADRs. See ADR-024 §Gap 9 for
the binary distribution and macOS packaging decision.
```