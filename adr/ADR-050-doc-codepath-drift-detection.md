# ADR-050: Doc-Codepath Drift Detection

**Status:** Proposed
**Chain:** ADR-028 (module sharding), ADR-049 (shared-prefix prompt caching)
**PR:** #435 (`work/vexcoder-transcript-ansi-overstrike`)

## Context

Agent runs that depend on the repository's documented file map repeatedly resolve symbols at obsolete paths and re-read files that the manifest fails to enumerate. The cost is two-fold: redundant tool rounds during context assembly, and incorrect base anchors during edit planning.

Two manifests describe the repository today. `CONTRIBUTING.md` carries an inline raw-URL table covering 96 paths. `TASKS/completed/REPO-RAW-URL-MAP.md` carries the canonical 417-row table validated by `.github/workflows/doc-ref-check.yml`.

A line-by-line audit of merged PRs #428, #429, #431, #433, #434 against both manifests yields the following invariants and breaches.

### Observed breaches

1. **Renamed module not propagated.** `src/api/eventsource{,/non_stream,/tests,/tests/local_retry,/tests/stream_lifecycle}.rs` (5 paths) appear in `REPO-RAW-URL-MAP.md` but no longer exist on disk. Their replacements `src/api/stream_ingress*` (5 paths) exist but are absent from the map. Path-set cardinality is unchanged, so the count-parity check in `doc-ref-check.yml` remains green while the path-set diff is non-empty.

2. **Sub-crate extraction not propagated.** `CONTRIBUTING.md` row for `src/types/api_types.rs` resolves to a path that has been removed; the symbol set now lives in `crates/vexcoder-api-types/src/lib.rs`. `src/types.rs` re-exports via `pub use vexcoder_api_types::*;`.

3. **Submodule fan-out not enumerated.** Of 23 distinct paths edited across PRs #428–#434, 21 are absent from `CONTRIBUTING.md`'s inline table. Examples: `src/state/conversation/tools/{formatting,routing,validation,tests}.rs`, `src/state/conversation/tests/{history,streaming,tool_execution,read_file_guard}.rs`, `src/api/stream_ingress.rs` and its peer files, `src/api/stream/text_normaliser.rs`, `src/runtime/json_handoff/protocol_ingress.rs`, `src/app/tests/{transcript,input}.rs`, `src/app/tests/model_turn/tool_rendering.rs`, `tests/live_server_test/tool_calls.rs`. The inline table enumerates parent module files only and does not index the peer files into which those modules have been factored.

4. **Tests-directory shape mismatch.** `CONTRIBUTING.md` row for `src/state/conversation/tests.rs` describes a flat file. The current shape is a directory `src/state/conversation/tests/` with `mod.rs` plus nine peer test modules.

5. **Recent ADR not indexed.** `adr/ADR-049-shared-prefix-prompt-caching-and-fork-controls.md` exists on `main` and is not in `REPO-RAW-URL-MAP.md`. Header count remains 417 because an unrelated path was removed in the same window, again masking the breach under count parity.

### Root cause

The repository's current invariant is path-set count, not path-set membership. A rename that preserves cardinality, or a removal balanced by an unrelated addition, evades the check. Documentation drift detection literature treats this exact pattern as a count-versus-content mismatch and recommends diff-driven membership comparison rather than scalar coverage [DDD-1, ARCH-1].

## Decision

### Normative requirements

R1. The canonical file manifest is `TASKS/completed/REPO-RAW-URL-MAP.md`. The inline table in `CONTRIBUTING.md` MUST be reduced to a load-bearing subset (entry points only) or removed; it MUST NOT enumerate paths the canonical manifest already covers.

R2. `doc-ref-check.yml` MUST be extended with a path-set membership check:

```bash
# Pseudocode for the membership pass
git ls-files | sort -u > /tmp/tracked
grep -oE 'aistar-au/vexcoder/main/[^>]+' TASKS/completed/REPO-RAW-URL-MAP.md \
  | sed 's|aistar-au/vexcoder/main/||' | sort -u > /tmp/mapped
diff /tmp/tracked /tmp/mapped
```

The job MUST fail when the symmetric difference is non-empty. The existing count-parity step is retained as a fast pre-check.

R3. Renames that move a module across paths MUST update `REPO-RAW-URL-MAP.md` in the same PR. The doc-ref-check failure mode SHOULD reference the offending PR by name.

R4. Sub-crate extractions (e.g., the api-types extraction in #429) MUST add a `crates/<name>/src/lib.rs` row and remove the prior `src/<area>/*.rs` rows in the same PR.

R5. Agent shard prompts in `CONTRIBUTING.md` (the prompts that begin "Shard:") MUST be regenerated from the canonical manifest at release boundaries; static path lists in those prompts are themselves drift surfaces.

### Non-goals

- This ADR does not change the manifest format or the raw-URL anchoring. Both remain as defined.
- This ADR does not amend the agent dispatch workflow.

## Rationale

A count-parity check is a necessary but not sufficient invariant. Two operations that preserve cardinality (rename, balanced add/remove) leave the manifest semantically wrong while leaving the workflow's signal green. The breaches above are evidence: each was authored under a green CI yet each broke an active agent path.

A membership check has the property that any symbol-bearing rename is forced to update the manifest by definition, because the symmetric difference is non-zero until the row moves. Membership is the smallest invariant that catches all observed breaches without introducing semantic understanding of file contents.

The choice of `REPO-RAW-URL-MAP.md` as canonical (over the inline `CONTRIBUTING.md` table) follows from the workflow's anchor: the workflow already validates that file. Two manifests with overlapping ownership and one CI anchor is a strictly weaker invariant than one manifest with the same anchor.

The literature on documentation drift treats freshness metrics (file age, count parity, time since last update) as proxies that fail under semantic change [DDD-1]. Architecture-drift work makes the same point at a coarser grain: divergence between described and realized structure accumulates silently until a downstream consumer (here, an agent) acts on the obsolete description [ARCH-1].

## Consequences

- A subsequent PR MUST land the membership check in `doc-ref-check.yml` and regenerate `REPO-RAW-URL-MAP.md` to cover the five `stream_ingress` paths, the api-types sub-crate row, and ADR-049. That PR is the natural place to also drop the inline table from `CONTRIBUTING.md`.
- The five `eventsource` rows MUST be removed in the same regeneration.
- Agent profiles whose prompts hard-code path lists become drift surfaces. Either the lists are regenerated from the manifest at release time, or the prompts are reduced to module-root references and a pointer to the manifest.

## References

- DDD-1: Doc-drift detection in CI as a diff-driven check rather than a freshness metric (`understandingdata.com/posts/doc-drift-detection-ci/`).
- ARCH-1: Architecture drift and erosion as the divergence between intended and realized structure (ScienceDirect, 2023; ACM ICISDM 2020).
- RAT-1: Ratatui FAQ on text rendering and ANSI handling (`ratatui.rs/faq/`), context for the parent PR.
