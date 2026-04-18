# ADR-022 Amendment — 2026-03-03

**Amendment status:** Proposed
**Amends:** ADR-022 Decision item 1 and the final Compliance note; adds Decision item 11
**Reason:** ADR-022 as written frames cli-agent-first and no-editor-integration as permanent identity statements. This amendment re-scopes them as first-release sequencing constraints, preserving architectural priority without permanently ruling out native application packaging or future editor surfaces.

---

## What changes

### Decision item 1 — amended

**Before:**
> `vexcoder` remains cli-agent-first, not editor-first.

**After:**
> `vexcoder` is cli-agent-first for phase 1. The cli runtime is the canonical execution surface and must remain so at every packaging layer. Native application packaging (e.g. a macOS wrapper) and editor-surface integration (e.g. a general editor extension) are not in scope for phase 1 and must not be allowed to drive architectural changes to the runtime core.

### Decision item 11 — added

> Native application packaging and additional runtime surfaces are reserved for work beyond the first release. When introduced, they must be implemented in one of two forms: (a) a *packaging layer* — wraps the compiled binary, adds OS-native credential storage and chrome, contains no agent logic; or (b) a *new `RuntimeMode` implementation* — implements `RuntimeMode + FrontendAdapter` against the shared runtime core, is placed in `src/` like `TuiMode` and `BatchMode`, and extends rather than replaces the existing dispatch architecture. A local HTTP or Unix socket API server (`LocalApiServer: RuntimeMode + FrontendAdapter`) is a canonical example of form (b): it is not a packaging layer, it is a new surface implementation, and it belongs in `src/` by design. The unsupported case is an *architectural fork*: a surface that requires changes to `src/runtime/`, `src/api/`, or `src/state/` to function, modifies the shared runtime core to serve its own needs, or duplicates runtime logic in a second language rather than sharing it through the internal runtime API surface.

### Final Compliance note — amended

**Before:**
> Do not convert the product into an editor-first application under this ADR.

**After:**
> Do not introduce native application packaging or new runtime surface implementations in the first release. Any subsequent phase that introduces these must do so via a dedicated ADR. Packaging layers must not contain agent logic. New `RuntimeMode` implementations must call into the shared runtime core unchanged — they must not modify `src/runtime/`, `src/api/`, or `src/state/` to serve surface-specific needs.

---

## Rationale

The original wording was written to prevent scope creep during phase 1, which was the correct intent. However, permanently ruling out native packaging and editor surfaces would make `vexcoder` harder to distribute and adopt — both of which are required for it to function as a viable, self-hostable coding agent whose dependency chain carries no per-call licensing fee or royalty obligation.

This amendment preserves the sequencing intent (cli core first, packaging layers second) while leaving room for:

1. **A macOS application wrapper (Phase H).** A wrapper that launches and manages the `vex` binary, provides OS-native credential storage, and presents a cli surface in an application window is a *packaging layer*. The Rust runtime runs unchanged inside it. This is a subsequent-stage macOS surface; it must not begin before the edit loop, approval system, and task state persistence are validated end-to-end.

2. **A full native macOS client (post-Phase H).** A native macOS application that communicates with a `LocalApiServer: RuntimeMode + FrontendAdapter` running in-process or as a local daemon is a *new surface implementation* over the shared runtime core. It is architecturally equivalent to how cloud API servers work — the transport may stay on private IPC or extend to TLS-protected TCP, but the interface contract is identical. This path enables a full-featured native macOS application without duplicating any Rust logic in the native layer. It requires a dedicated ADR and must not begin before `BatchMode` and the core correctness gate are validated end-to-end.

3. **A future editor surface.** An extension that shells out to `vex exec` and renders JSONL output in a panel is a thin editor surface over an unmodified cli runtime, provided the extension never owns the agent loop itself.

The critical architectural constraint preserved by this amendment:

> **The shared runtime core — `src/runtime/`, `src/api/`, `src/state/` — must remain the single canonical implementation across all surfaces. A new surface that requires *modifications* to these modules to function is an architectural fork and must be treated as such. A new surface that adds a new `RuntimeMode + FrontendAdapter` implementation and calls into the existing core unchanged is an intended use of the runtime module API — `TuiMode`, `BatchMode`, and `LocalApiServer` are parallel implementations of the same shared engine, not forks of it.**

---

## What does not change

- The first-release scope is unchanged. No packaging or editor work in the initial phase.
- `vex exec` headless mode (ADR-024 Gap 2 / `BatchMode`) is the designated integration point for any future editor surface. It must be stable before an editor extension is designed.
- `RuntimeCorePolicy` and `ApprovalPolicy` remain separate concerns.
- All other ADR-022 Decision items (2–10) are unchanged.
- All other ADR-022 Compliance notes are unchanged.

---

## Application instructions

Apply by editing `adr/ADR-022-free-open-coding-agent-roadmap.md` in place:

1. Replace Decision item 1 with the amended text above.
2. Append Decision item 11 after item 10.
3. Replace the final Compliance note with the amended text above.
4. Add a dated amendment header immediately after the `**ADR chain:**` line:

```
**Amendment:** 2026-03-03 — Decision item 1 and the final Compliance note
scoped to the first release; Decision item 11 added to reserve native packaging
and editor surfaces for later-stage ADRs. See ADR-024 §Gap 9 for
the binary distribution and macOS packaging decision.
```
