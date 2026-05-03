# ADR-047 Amendment (2026-05-04): PR #429 Iteration Post-Mortem and What-Not-To-Do

**Status:** Amended
**Amends:** ADR-047, ADR-047-amendment-2026-04-16, ADR-047-amendment-2026-04-20, ADR-047-amendment-2026-05-01, ADR-047-amendment-2026-05-03
**Follow-up to:** PR #429

## Purpose

PR #429 ("Remove tagged tool-call fallback seam") shipped 15 commits over multiple sessions. The branch was merged with six failing CI checks, including three `clippy` errors and a stale repo-map header that any agent running the standard local-gate sequence would have caught before pushing. The same failure surfaces also reproduce in the live local-endpoint run that motivated the work in the first place.

This amendment records the per-commit attempt → failure → next-attempt narrative so that the next agent touching the structured/XML tool-call seam, the loop-guard family, or the `send_message` outer loop does not re-discover the same blind alleys. Read the **What not to do** section first; it is the highest-leverage payoff per minute spent.

## Per-commit narrative

| # | SHA       | Intent                                                            | Why it was insufficient                                                                                  | Resolved by             |
|---|-----------|-------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------|-------------------------|
| 1 | f6c4393   | Remove the tagged-XML tool-call fallback seam (the goal of #429)  | Local models that emit only XML-encoded tool calls had no path to call tools at all                      | #2 (`c194db4`)          |
| 2 | c194db4   | Normalize tagged XML at ingress (`text_normaliser` + `protocol_ingress`) | clippy lint                                                                                              | #3 (`e99ff5f`)          |
| 3 | e99ff5f   | Fix ingress-normalizer clippy lint                                | Only handled parameter-tagged XML; missed Hermes JSON and outer-wrapper format used by the test target   | #4 (`5031c0a`)          |
| 4 | 5031c0a   | Add Hermes JSON + outer-wrapper formats to the normalizer         | Tool-call card opened with empty input → TUI rendered `path: <missing>`                                  | #5 (`63160cc`)          |
| 5 | 63160cc   | Re-emit `BlockStart` from inside `ToolCallArgumentsDelta` once arguments fully parse | Wrong fix location; only papered over the symptom and later caused **duplicate TUI cards**. Reverted in #14 | #14 (`1a6b841`)         |
| 6 | cebcf4d   | Materialize accumulated arguments at block-open time inside `protocol_ingress` | Wrong layer; broke argument materialisation downstream. Reverted in #8                                   | #8 (`2c8d750`)          |
| 7 | 2e1902b   | Fix `clippy::manual_inspect` regression from #6                   | Lint-only — the underlying logic from #6 was still wrong                                                 | #8 (`2c8d750`)          |
| 8 | 2c8d750   | Revert early-parse, suppress `path:<missing>` via empty-input guard in `tool_preview` | Local XML-only models still received `ContentBlock::ToolUse`/`::ToolResult` history they could not parse | #9 (`9fd4c8f`)          |
| 9 | 9fd4c8f   | Restore text protocol when `is_local_endpoint() && !saw_native_tool_call_block` | Worked, but loop guard only caught **consecutive** repeated read-only signatures                         | #10 (`2f9f64a`)         |
| 10 | 2f9f64a  | HashSet-based signature accumulation across the whole turn        | Missed the **empty-input** `read_file` failure mode under context pressure (model restarts at offset 0) | #11 (`7cd11bd`)         |
| 11 | 7cd11bd  | Track `last_read_file_path` and enrich the empty-input clarification message | Compile error: `Value::as_str` referenced as a method pointer without importing `serde_json::Value`     | #13 (`bb8f5f7`)         |
| 12 | 1607986  | ADR amendments documenting #1–#11                                 | (No regression)                                                                                          | —                       |
| 13 | bb8f5f7  | Replace method-pointer with closure form `\|v\| v.as_str()`         | Fixed compile, but two **`if X { if let Y = … }`** nestings landed that trip `clippy::collapsible-if`   | This amendment          |
| 14 | 1a6b841  | Defer `BlockStart` to `ToolCallStarted` so the runtime ID is used | Removed the duplicate-card regression introduced by #5                                                   | (final form)            |
| 15 | 322ccf1  | Inject `final_answer_instruction` before max-tool-rounds termination | Added a third `if X { if let Y = … }` nesting (also caught by `clippy::collapsible-if`)                | This amendment          |

The follow-up CI fixes that landed after merge (this amendment's PR) collapse the three nestings into single `if` chains using `let` chain syntax, refresh the repo raw-URL map to 417 entries, and add the `RUSTSEC-2026-0119` (hickory-proto DoS) advisory exception that was never accepted on the branch.

## What not to do

The bullets below are the concrete blind alleys that cost the most time on PR #429. None of them are caught by language-level checks; all of them are catchable by either local lint gates or by reading the right ADR before editing.

### About loop guards (`send_message.rs::send_message_with_policy`)

- **Do not patch `ToolCallArgumentsDelta` to re-emit `BlockStart` to fix `path: <missing>`.** That is the path commit `63160cc` took. The downstream TUI duplicate-check matches by ID; the synthesized `toulu_tagged_*` source ID and the runtime `tx_*` ID are not equal, so the second `BlockStart` creates a second `PulseEntry::ToolCall` and the user sees two cards. The correct location is the `TranscriptBlockStart { ToolCall }` arm: hold the index in `pending_tool_block_indices` and emit one `BlockStart` from the `ToolCallStarted` arm with the runtime ID. See `ADR-047-amendment-2026-05-03.md` for the working diff.
- **Do not materialize tool-call arguments inside `protocol_ingress`.** That was commit `cebcf4d`. Argument deltas accumulate in `ToolCallArgumentsDelta`; the ingress layer must stay JSON-shape-agnostic.
- **Do not use consecutive-only loop-guard comparison.** The original `previous_round_signature` only caught back-to-back repeats. A model returning to the same signature after one different round (a search wedge between two reads of offset 0) silently restarts the counter. Use the `seen_read_only_signatures: HashSet<Vec<String>>` accumulator. See `ADR-023-amendment-2026-05-01.md`.
- **Do not let the budget guard terminate without a final-answer attempt.** `rounds > max_tool_rounds` previously returned the loop-limit message verbatim. The model never got a chance to summarise the evidence it had already gathered. Inject `final_answer_instruction()` from `RuntimeCorePolicy` once before terminating; let the model emit text-only on the next round; keep the original termination as the second-strike fallback. See `ADR-023-amendment-2026-05-03.md`.
- **Do not enrich the empty-input clarification message without remembering the last `read_file` path.** The clarification ("I need an explicit file path. Please call read_file with a 'path' argument.") gives the model nothing to continue from when the model just emitted an empty `read_file` because of context pressure. Track `last_read_file_path` in both the parallel and serial execution paths and append " You were most recently reading 'X' — specify that path to continue." See `ADR-032-amendment-2026-05-01.md`.

### About the structured-tool-call protocol seam

- **Do not gate `use_structured_round` on `use_structured_tool_protocol` alone.** Local models that produce only XML-encoded tool calls cannot interpret the structured `ContentBlock::ToolUse`/`::ToolResult` history the runtime would otherwise replay. Use `use_structured_tool_protocol && (!is_local_endpoint() || saw_native_tool_call_block)`. `saw_native_tool_call_block` is set inside the turn when any `ToolCall` block opens with a server-assigned ID (an ID that does *not* start with `toulu_tagged_`). See `ADR-047-amendment-2026-05-01.md`.
- **Do not delete the tagged-XML normalizer just because the seam is gone.** The seam was the consumer-side rewrite (TUI/batch). Ingress still has to convert XML/Hermes/outer-wrapper tool calls into `ToolCallStarted` signals. The normalizer lives under `src/api/stream/text_normaliser.rs` and `src/runtime/json_handoff/protocol_ingress.rs`.

### About local gates that would have caught the merge-time CI failures

Run all of these locally before pushing — the entire suite finishes in well under a minute on a warm cache:

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo deny check
bash scripts/check_forbidden_names.sh
EXPECTED=$(git ls-files | wc -l | tr -d ' ')
HEADER=$(grep 'Total tracked files:' TASKS/completed/REPO-RAW-URL-MAP.md | grep -oE '[0-9]+' | head -1)
[ "$EXPECTED" = "$HEADER" ] || echo "regenerate REPO-RAW-URL-MAP.md ($HEADER → $EXPECTED)"
```

`make gate-fast` runs the first two plus `nextest`. PR #429 merged with `clippy`, `windows-clippy`, `arch-check`, `cargo-deny`, `lint`, and `check-map-coverage` red because none of these were re-run after the late commits (`bb8f5f7`, `1a6b841`, `322ccf1`).

- **Do not rely on `cargo check` to catch lint regressions.** `cargo check` does not run clippy; clippy ran red from commit `7cd11bd` onward and never went green on the branch.
- **Do not write `if X { if let Y = … { … } }` in modern Rust.** Use the let-chain form `if X && let Y = … { … }`. Clippy's `collapsible-if` lint is denied via `-D warnings` in CI.
- **Do not reference `Value::as_str` as a method pointer.** Use the closure form `|v| v.as_str()`. `serde_json::Value` is not always in scope at the call site, and the implicit method-pointer form requires it.
- **Do not paste vendor product names into ADR docs.** `scripts/check_forbidden_names.sh` blocks brand tokens (assistants, model families, runtimes) and several tone words (notably the one meaning "final" that begins with "term"). Use neutral wording: "OAS-compatible HTTP endpoint", "a 25B-parameter coder model", "completion state" instead of "final state" using that blocked word. The full block list lives in the script's `brand_words` and `tone_words` arrays.
- **Do not forget to regenerate `TASKS/completed/REPO-RAW-URL-MAP.md` when adding tracked files.** `check-map-coverage` compares `git ls-files | wc -l` against the `Total tracked files: N` header line and fails the workflow if they differ. Five new ADR files added without a header bump caused the failure on PR #429.
- **Do not add a transitive-dep RustSec advisory ignore inline in `deny.toml` without a `reason =` describing the dependency path, the affected API, and the resolution trigger.** `cargo-deny` rejects bare ignores. The advisories block in `deny.toml` is the canonical place; resolve advisories upstream where possible, and only document an exception when the affected API surface is genuinely unreachable from this codebase.

## Cost of getting this wrong

PR #429 took ~8 hours of agent time to produce 15 commits, several of which were straight reverts, and still merged with six red checks. The functional behaviour shipped on the branch is now stable, but it would have been stable in roughly two commits — one ingress-side fix and one outer-loop loop-guard amendment — if the lint and format gates had been honoured at every push. This amendment exists so the next agent can skip directly to the `What not to do` section before opening the editor.
