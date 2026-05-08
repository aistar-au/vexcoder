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

## Amendment 2026-05-08 — PR #435 follow-through and remediation map

Closes the manifest breaches enumerated in §"Observed breaches" and records the exact code-path locations for the five harness-level remediations identified in the PR-435 debug report. The intent of this amendment is to remove rediscovery cost from any subsequent agent or contributor: each follow-up is named by file and (where stable) line range so that no exploration phase is required.

### Manifest membership — closed in this PR

- `TASKS/completed/REPO-RAW-URL-MAP.md`: the five `eventsource` rows (157–161) are replaced by their `stream_ingress` successors at the same row indices. Six previously absent paths are inserted at fractional indices `54a`, `54b`, `63a`, `79a`, `79b`, `81a` (ADR-049). The pre-existing ADR-050 row is renumbered from `81a` to `81b`. The path-set symmetric difference against `git ls-files` is now empty (verified by `comm -3`).
- The api-types sub-crate row noted in §"Observed breaches" claim 2 is already present in the canonical manifest under its actual path `crates/vexcoder-api-types/src/lib.rs`; no further action is required for that row in this PR.

### R1 — Transcript renderer + hermetic test (closed in this PR)

- `src/ui/render/transcript.rs:287`: `looks_like_inline_markdown` now matches CommonMark Spec 0.30 §4.2 — accepts 1–6 `#` characters followed by space, tab, or end-of-line; rejects ≥7 hashes. Verified by §4.2 examples 62, 63, 79.
- `tests/live_server_test.rs`: the live-server-dependent stream test is replaced by an in-process `axum` mock (`spawn_messages_v1_stream_server`, `streaming_messages_v1_text_handler`) emitting the standard SSE envelope sequence. Pattern follows `tokio-rs/axum/examples/testing/src/main.rs`.

### R5 (partial) — Read-file anchor extended with offset (closed in this PR)

- `src/state/conversation/send_message.rs:62`: `last_read_file_path: Option<String>` is replaced by `last_read_file_anchor: Option<(String, Option<u64>)>`. The corresponding storage sites at lines ~605 and ~824 capture `input.get("offset").and_then(serde_json::Value::as_u64)`. The clarification site at lines ~651–656 emits `at offset N` when the anchor's offset component is `Some`. This closes the post-compaction "empty path retry" failure mode observed in the PR-435 debug transcript.

### Deferred follow-ups with named edit sites

The remaining recommendations are scoped to subsystems outside this PR's stated boundary. Each is recorded here with an exact entry point so a follow-up PR can act without rediscovery.

- **R2 — Span-aware wrapping**. Anchor: `src/ui/render/transcript.rs` `word_wrap_transcript_row` and `word_wrap_plain_row`. Current behaviour: early return when `contains_ansi_escape(text)` is true. Target behaviour: flatten the styled `Line` into a string while recording per-span byte ranges, wrap under a `WordSplitter` that does not split CSI sequences, then reconstruct styled spans from the recorded ranges. The same pattern is implemented in an unrelated upstream Rust TUI agent project's `tui/src/wrapping.rs` (function names commonly used: `word_wrap_line`, `adaptive_wrap_line`, `slice_line_spans`); a URL-clickability variant of the same flatten-then-reconstruct strategy has also been published. Adopt the structural approach, not any verbatim symbols.
- **R3 — Markdown discrimination over time**. Anchor: `src/ui/render/transcript.rs:295–308` (`has_paired_marker`, `has_paired_backtick`). Target: replace the heuristic with a single inline-only pass through `pulldown-cmark` (already implied by the parent `super::markdown_to_inline_line` call) so that fenced code, blockquotes, and list bullets are handled by the same parser that renders them. The current heuristic admits no false negatives that the parent renderer would reject downstream, so this is an optimisation rather than a correctness gap.
- **R4 — Result-aware loop detection**. Anchor: `src/state/conversation/send_message.rs:56–60` — the existing fields `previous_round_signature`, `seen_read_only_signatures`, `repeated_read_only_rounds` already track tool-call signatures across rounds. Target: extend the signature to a hash of `(tool_name, canonicalised_input, canonicalised_output)` and inject a corrective system message after `repeated_read_only_rounds` exceeds a configurable threshold (proposed default: 3). The model-vendor's published tool-use guidance describes an equivalent automatic tool-result clearing mechanism on the upstream API; this ADR makes the same control available to the local-server path.
- **R6 — Token-budgeted read_file**. Anchor: the `read_file` tool registration at `src/export.rs:180` and the input shape consumed in `src/tool_preview.rs:272–280`. Target: accept an optional `max_tokens` parameter that bounds the returned slice to the largest contiguous region fitting that budget, and return a sentinel `next_offset` when truncated. This eliminates the eight-round paginated-read pattern observed in the PR-435 debug transcript against `release.yml` (509 lines, 17 041 bytes). Required reciprocal change: tool schema in `src/export.rs` and the tool dispatcher under `src/state/conversation/tools/`.
- **R7 — Cache-prefix invariant**. Anchor: ADR-049 (`adr/ADR-049-shared-prefix-prompt-caching-and-fork-controls.md`). Target: assert the invariant that no message-mutation operation (compaction, undo, prune) may rewrite blocks at or before the active cache breakpoint inside the 5-minute or 1-hour TTL window. The upstream prompt-caching documentation specifies this constraint as a 100-percent identical-prefix requirement up to and including the cache-control breakpoint, with a 20-block lookback. Compaction sites of interest are `condense_old_tool_results` (called from `send_message.rs:67`) and the prune anchor logic introduced in PR #433 (commit `df7913e`).

### References (added)

- ATX-1: CommonMark Spec 0.30 §4.2 ATX headings, examples 62, 63, 79 (`spec.commonmark.org/0.30/`).
- WRAP-1: An upstream Rust TUI agent project's `tui/src/wrapping.rs` — functional outline of flatten-record-wrap-reconstruct; URL-aware adaptive variant published as a separate refinement.
- ANSI-1: ratatui/ansi-to-tui README — SGR coverage; cursor controls and `\r` not interpreted; callers must strip upstream.
- AXUM-1: tokio-rs/axum testing example (`axum/examples/testing/src/main.rs`); axum-test crate (mock transport idiom).
- LOOP-1: Liu et al., *Lost in the Middle: How Language Models Use Long Contexts* (arXiv:2307.03172) — context-position degradation.
- LOOP-2: The model vendor's *advanced tool use* guidance — automatic tool-result clearing for long-session loop mitigation.
- CACHE-1: Upstream prompt-caching documentation — 5-minute / 1-hour TTLs, 20-block lookback, prefix-invariance rules.
