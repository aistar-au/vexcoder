# ADR-023 Amendment (2026-05-03): Final-Answer Fallback at Max-Tool-Rounds Termination

**Status:** Amended
**Amends:** ADR-023, ADR-023-amendment-2026-05-01
**PR:** #429 (`work/vexcoder-remove-tagged-xml-fallback`)

## Amendment

### Termination without final answer (live-test reproduction)

ADR-023 established a `max_tool_rounds` cap (12 for local endpoints, 24 otherwise) with the rationale that no productive turn requires more than that many tool calls. ADR-023-amendment-2026-05-01 then strengthened the read-only loop guard with HashSet-based signature accumulation so that any-distance signature repetitions are caught.

Live testing against a local OAS-compatible endpoint (a 25B-parameter coder model loaded with a 16384-token context) reproduced a failure mode that neither guard catches and that the cap alone does not handle gracefully:

```
read_file("release.yml", offset=0,   limit=80)   → round 1
read_file("release.yml", offset=80,  limit=80)   → round 2
read_file("release.yml", offset=160, limit=80)   → round 3
read_file("release.yml", offset=240, limit=80)   → round 4
... continuing through 509 lines ...
read_file("build-app.sh", offset=0,  limit=80)   → round 8
read_file("Info.plist",   offset=0,  limit=80)   → round 11
read_file("release.yml", offset=400, limit=80)  → round 12
[loop guard] Stopped after 12 tool rounds to prevent an infinite loop.
```

Each call has a unique `(name, input)` tuple, so the HashSet signature guard never fires. The model is paginating legitimately — it is not stuck — but the tool budget runs out before the model produces a user-facing answer. The previous implementation returned the loop-limit message verbatim with whatever transient assistant text happened to be in flight (typically a fragment such as "Let me read the file…"), giving the user no useful response.

### Fix: one-shot final-answer prompt before termination

A new outer-loop flag `final_answer_attempted: bool` is added. The `rounds > max_tool_rounds` branch becomes:

```rust
if rounds > max_tool_rounds {
    if !final_answer_attempted {
        final_answer_attempted = true;
        self.api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: Content::Text(
                core_policy.final_answer_instruction().to_string(),
            ),
            cache_hint: None,
        });
    } else {
        // existing termination path (render guard message, return)
    }
}
```

`final_answer_instruction()` is a new `RuntimeCorePolicy` method that returns:

> "You have reached the tool-call budget for this turn. Stop calling tools and write your final answer now using the information already gathered. Summarize the relevant findings and respond directly to the user's request."

Behaviour after the change:

- **Round `max + 1`**: the budget-exceeded branch fires, the flag is set, the prompt is appended, control falls through to the normal model-call code. The model is asked to summarise without tools.
- **Model emits text only**: the existing `tool_use_blocks.is_empty()` branch returns the assistant text as the turn's response — the user gets a real answer instead of "Stopped after N tool rounds".
- **Model still emits tool calls**: those calls execute (the prompt is advisory, not enforced), and the next iteration enters the `else` branch with `final_answer_attempted == true`, which renders the original loop-limit message and terminates. The previous behaviour is preserved as the fallback when the model ignores the prompt.

The flag is per-turn (declared inside `send_message`'s outer loop) and is not persisted across turns. Each new user message resets it.

### Relationship to other guards

- The signature-based loop guard (ADR-023-amendment-2026-05-01) still fires first when the model genuinely repeats a signature; the final-answer fallback only handles the distinct case of legitimate progress hitting the budget.
- The repeated-round nudge (`repeated_tool_round_instruction`) and the final-answer prompt are independent: the nudge fires after one signature repetition and asks the model to vary its approach; the final-answer prompt fires after the round budget is exhausted and asks the model to stop calling tools entirely.
- The mutating-round guard (`repeated_mutating_rounds`) is unchanged and still terminates immediately on the first repeated mutating signature.

### Live verification

A reproduction was run against a local OAS-compatible inference server (a 25B-parameter coder model, 16384-token context, CPU-only) with `VEX_MAX_TOOL_ROUNDS=3` to compress the budget so the fallback fires quickly:

```
$ VEX_MODEL_URL=http://0.0.0.0:8000/v1/messages \
  VEX_MAX_TOKENS=16384 \
  VEX_MAX_TOOL_ROUNDS=3 \
  ./target/debug/vex -f -p "read-only do not modify and debug .github/workflows/release.yml" -m jsonl exec
```

Inference timeline (from the local inference server logs):

```
POST /v1/messages — round 1: read_file(release.yml, offset=0)   →   91 s
POST /v1/messages — round 2: read_file(release.yml, offset=80)  →  103 s
POST /v1/messages — round 3: read_file(release.yml, offset=160) →  119 s
POST /v1/messages — round 4: final-answer prompt injected,
                              479-token text response generated  →  233 s
status: Completed  total_turns: 1
```

JSONL output (truncated):

```json
{"pulse":1,"input":"read-only do not modify and debug .github/workflows/release.yml",
 "response":"I'll help you read and analyze the `.github/workflows/release.yml` file…\n\n# Analysis of .github/workflows/release.yml\n\nThis is a GitHub Actions workflow file…\n## Key Jobs\n1. **release-gate** (Line 22): …\n2. **build** (Line 76): …\n3. **sign** (Line 205): …\n## Release Tag Validation\n…\nThis workflow automates the entire release process from building binaries to signing and publishing releases, with proper validation at each step.",
 …"tokens":{"input":2689,"output":620,…}}
{"summary":true,"status":"Completed","task_id":"batch-1777770925011","total_turns":1,…}
```

Pre-fix behaviour for the same scenario produced `status: MaxTurnsReached` and `response: "Let me read the file to understand its contents.\n\n[loop guard] Stopped after 3 tool rounds to prevent an infinite loop."` — the model's intermediate thinking plus the termination notice, with no usable answer. After the fix, the same model on the same budget returns a structured, evidence-backed analysis of the file the user asked about.

### Follow-up: second-turn handoff compaction for local read-only sessions (2026-05-04)

Live testing against a local OAS-compatible endpoint with an `8192`-token context window surfaced a separate failure after the max-tool-round fallback was fixed. A first read-heavy turn on `.github/workflows/release.yml` could complete, but the next read-only turn on `.github/workflows/nightly.yml` could still inherit too much literal history from the prior turn. In that state the model sometimes resumed the earlier release-workflow analysis instead of treating the nightly workflow as the active target.

### Fix: proactive one-turn handoff compaction before local read-only follow-ups

When the operator is using the default compaction configuration on a local endpoint, any read-only follow-up turn that still has retained history now collapses the older conversation into a single handoff summary before the next model call. The compactor keeps only the current user turn, caps the heuristic summary at `384` tokens, and rewrites the surviving prompt into this shape:

```text
[conversation summary] ...

Use the summary only as background context. Treat the current request below as authoritative and do not continue earlier file targets unless the current request asks for them.

[current request]
<current user prompt>
```

This is intentionally stricter than the generic overflow compactor. The goal is not only to fit inside the server window; it is also to keep the current file target more salient than the previous turn's raw tool transcript on smaller local models.

The overflow retry path now preserves the active user-turn anchor as well. That matters when a proactive handoff is followed by another read-heavy tool round: the retry compactor must be allowed to condense tool-result payloads, but it must not drop the current file request just because the tail of the transcript contains only assistant/tool chatter. The heuristic handoff summary is also limited to prior user requests and user-side tool evidence so stale assistant headings do not leak into the next file's answer title.

### Edit the owning files first

The high-leverage edit surface for this failure is small and local:

- `src/state/conversation/history.rs` owns proactive compaction policy, summary insertion, and the surviving prompt shape.
- `src/state/conversation/send_message.rs` is the caller that decides when the turn enters the proactive compactor.
- `src/state/conversation/tests/history.rs` is the narrow regression surface for the second-turn handoff path.
- `adr/ADR-023-amendment-2026-05-03.md` is the correct architecture note to record the runtime contract change and the live validation evidence.

Files such as `.github/workflows/*.yml`, `scripts/release.sh`, `scripts/bump-version.sh`, `src/app/input.rs`, and `src/runtime/context.rs` are useful for reproducing the symptom, but they are not the controlling implementation surface for this bug. Editing those first burns time without changing the second-turn compaction decision.

### External rationale

This follow-up aligns with two publicly documented agent-design patterns:

- A public terminal-agent prompting guide states that long-running threads should compact context by summarizing relevant prior state and discarding less relevant detail so the agent can continue across many steps without replaying the full transcript.
- A public function-calling guide describes the tool loop as an explicit developer-managed sequence of: model selects a tool, the developer executes it, the tool result is fed back, and the cycle repeats until the model produces a final answer. That structure favors concise handoff state over verbatim replay of earlier rounds.

Together, those references support a runtime policy that preserves the current request verbatim while reducing older turns to a bounded handoff summary.

### Live verification

A three-turn live probe against `http://127.0.0.1:8000/v1/messages` in the detached worktree produced the following post-fix sequence:

- first turn: overflow recovery condensed a read-heavy `release.yml` audit without dropping the active request.
- second turn: proactive handoff compaction reduced the carried state to a single prompt before the `nightly.yml` request (`17 -> 1` messages), and the response stayed on `.github/workflows/nightly.yml`.
- third turn: another proactive handoff reduced the carried state before the `version-bump.yml` request (`7 -> 1` messages), and the response stayed on `.github/workflows/version-bump.yml` rather than drifting back to `release.yml` or `nightly.yml`.

Before this follow-up, the same multi-file path could either answer from the older release-workflow context or hit the read/search loop guard after resuming the wrong file family on later turns.
