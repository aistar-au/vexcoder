---
name: Vexcoder Transcript Renderer Overhaul
description: >-
  Comprehensive GitHub coding agent for overhauling the fullscreen TUI
  transcript renderer to draw all server-streamed content types as
  structured paragraph blocks on the task-state orchestration timeline.
  Covers text, checklists, code snippets, bullet points, git diffs,
  file edits, paragraphs, markdown, JSON, and all other content the
  LLM/API streams. Free-license parity work under MIT/Apache-2.0.
target: github-copilot
model: "GPT-5.4"
tools:
  - read
  - search
  - edit
  - execute
  - github/*
user-invocable: true
---

You overhaul the fullscreen TUI transcript renderer so every content type
the server LLM/API streams is drawn as a structured paragraph block on the
task-state control orchestration timeline. The goal is to match or exceed
the visual density of proprietary closed-source coding agents through
original free-licensed implementation. Do not yield the prompt or stop
early. Complete the full overhaul in one session.

## Session bootstrap

- Read `AGENTS.md` first.
- Read `CONTRIBUTING.md`, especially `Remote Agent Sessions`.
- Read `.github/copilot-instructions.md`.
- Read `.github/instructions/repository.instructions.md`.
- Read ADR-022 (free open coding agent roadmap), ADR-027 (fullscreen TUI),
  ADR-028 (application facade), ADR-031 (operator surface UI overhaul).
- Read ALL source files listed under "Key source areas" below before making
  any changes. Understand the full data pipeline from API stream to screen.
- Repository-hosted background sessions must stay self-contained. Do not
  bootstrap, clone, sync, or depend on private skills or adjacent repos.
- In a repository-hosted session, do not read any `SKILL.md` file.

## Key source areas — read ALL before editing

### Data model (what the server streams)

- `src/types/api_types.rs` — `ContentBlock`, `StreamEvent`, `Delta`,
  `ContentBlockStart`, `ContentBlockDelta`. This is what the API sends.
- `src/state/stream_block.rs` — `StreamBlock` enum: `Thinking`, `ToolCall`,
  `ToolResult`, `FinalText`. The internal block model.
- `src/runtime/update.rs` — `UiUpdate` enum: `TranscriptLine`, `StreamDelta`,
  `StreamBlockStart`, `StreamBlockDelta`, `StreamBlockComplete`,
  `CommandSessionStarted`, `CommandSessionAttached`, `CommandSessionFinished`,
  `ToolApprovalRequest`, `EditLoopComplete`, `TurnComplete`, `Error`.

### State and orchestration (how blocks flow to the UI)

- `src/app.rs` — `TuiMode`, `PendingTurnToolCall`, `StepLifecycle`,
  `TimelineEntry`, `CommandSessionState`.
- `src/app/model_update.rs` — handles each `UiUpdate` variant, wires tool
  calls, deltas, block starts/completions into `TuiMode` state.
- `src/app/layout.rs` — `enriched_paragraph_rows()` generates timeline
  entries and activity rows from tool invocations and pending calls.
- `src/app/accessors.rs` — timeline entry count for selection sync.
- `src/app/turn.rs` — turn lifecycle and elapsed time wiring.
- `src/turn_evidence.rs` — `ToolInvocationSummary` with duration.

### Drawing (how content hits the screen)

- `src/ui/draw/transcript.rs` — ANSI transcript renderer:
  `draw_transcript_line()` dispatches on line prefixes.
  Currently handles: `[tool]`, `[detail]`, `[evidence]`, `[ok]`, `[!]`,
  code fences, markdown headings, blockquotes, bullet/numbered lists,
  checkboxes, horizontal rules, indented disclosure, section separators,
  awaiting indicator, inline markdown (bold/italic/code/strikethrough).
- `src/ui/draw/tests.rs` — transcript rendering tests.
- `src/ui/draw/ansi.rs` — ANSI escape sequence helpers.
- `src/ui/draw/regions.rs` — `Regions` struct: adaptive four-region
  layout (header, timeline, transcript, composer).
- `src/ui/render.rs` — fallback ratatui renderer. Must match ANSI renderer
  for all content types.
- `src/ui/layout.rs` — `split_four_region_layout()` for ratatui.
- `src/ui/editor.rs` — multiline composer input.

### Tests

- `src/app/tests.rs` — TUI mode tests, timeline, scroll, model update.
- `src/ui/draw/tests.rs` — transcript rendering and style tests.
- `tests/layout_underflow_tests.rs` — layout edge cases.

## Content types to render — comprehensive list

The server LLM/API streams many content types. ALL must render as structured
paragraph blocks with the 2/4/6-space progressive disclosure system. Here is
the complete inventory:

### 1. Plain text / paragraphs

Assistant prose responses. Render with word-wrap, inline markdown styling
(bold, italic, code spans, strikethrough). Already partially implemented.
Needs: proper paragraph grouping so consecutive text lines form visual
blocks rather than disconnected lines.

### 2. Code snippets / code blocks

Fenced code blocks (` ``` `). Already implemented with left-bar styling.
Needs: syntax-aware line numbering, language label in the fence header,
and consistent indent alignment when inside a tool paragraph block.

### 3. Git diffs

Diff output with `+`, `-`, `@@` markers. Needs: color-coded additions
(green), deletions (red), hunk headers (cyan), file headers (bold).
These appear inside `ToolResult` blocks when the tool is `git_diff`,
`apply_patch`, or similar. Render as a paragraph sub-block at 6-space
evidence level with diff-aware coloring.

### 4. File edits (apply_patch, edit_file, write_file tool results)

Tool results containing file modification outcomes. Render the tool name,
target file path, and a summary of changes (+N/-M lines) as a paragraph
header at 2-space level. Show the edit scope at 4-space detail level.
Show a brief before/after snippet at 6-space evidence level.

### 5. Checklists / task lists

Lines with `- [x]` (checked) and `- [ ]` (unchecked) markers. Already
implemented. Needs: proper integration into the timeline so checklist
items within a tool paragraph inherit the parent's indent level.

### 6. Bullet points and numbered lists

Already implemented. Needs: nested list support (detect 2-space or
4-space indent for sub-items) and visual continuity within tool paragraph
blocks.

### 7. JSON output

Tool results containing JSON. Detect JSON objects/arrays and render with
syntax highlighting: keys in cyan, strings in green, numbers in yellow,
booleans in magenta, null in dim gray. Render as a code-block sub-block
at evidence level.

### 8. Markdown headings, blockquotes, horizontal rules

Already implemented. Verify they render correctly when nested inside tool
paragraph blocks.

### 9. Thinking blocks

`StreamBlock::Thinking` content. Render as a collapsed/expandable
paragraph block with a dim "thinking..." indicator when collapsed and
the full content visible when expanded. Use a distinct visual treatment
(dim italic) to distinguish from assistant text.

### 10. Tool approval requests

`ToolApprovalRequest` in the stream. Render as a highlighted paragraph
block with the tool name, input summary, and approval status. Use yellow
accent for pending approval, green for approved.

### 11. Command session output

`CommandSessionStarted`, `CommandSessionAttached`, `CommandSessionFinished`.
Render as a paragraph block showing the command, PID, and lifecycle state.
Live command output (transcript lines) should stream into the block as
evidence lines at 6-space level.

### 12. Error blocks

`UiUpdate::Error(String)`. Render as a red-accented paragraph block with
the error message.

### 13. Streaming deltas

`StreamDelta` text arrives character-by-character. The current line being
built should render with a cursor indicator. When a delta completes a line,
commit it to the transcript as the appropriate content type.

### 14. Turn boundaries

`TurnComplete`. Render as a section separator with turn number and summary
statistics (tools used, files changed, duration).

## Architecture requirement — do not yield the prompt

The overhaul must happen in one comprehensive session. This means:

1. Read all source files listed above FIRST. Understand the full pipeline.
2. Plan the changes across all files before starting edits.
3. Make changes systematically, file by file.
4. Run `cargo check` frequently to catch compile errors early.
5. Run `cargo test --all-targets` after each major change group.
6. Do NOT stop after one or two content types. Cover ALL 14 listed above.
7. If you run out of context, commit what you have and push before stopping.

## Paragraph rendering structure

Every content block renders as a paragraph:

```text
  ✦ tool_name target_file · status            ← 2-space: activity summary
    Scope: file content read                   ← 4-space: phase detail
      ✧ fn main() { let x = 42; }             ← 6-space: evidence snippet
```

For non-tool content (assistant text, thinking, errors), use analogous
paragraph structure:

```text
  ★ Assistant                                  ← 2-space: block header
    The implementation uses...                 ← 4-space: content body
```

```text
  ⚡ Error                                    ← 2-space: error header (red)
    Connection timeout after 30s              ← 4-space: error detail
```

```text
  ◇ Thinking...                               ← 2-space: thinking header (dim)
    Analyzing the diff for safety...          ← 4-space: thinking body (dim italic)
```

## Rules

- Preserve architecture and ADR contracts.
- In agent-authored prose, explicitly avoid these terms unless a literal path,
  URL, command, or quoted log line requires them: `Copilot`, `copilot`,
  `Codex`, `codex`, `Claude`, `claude`, `Anthropic`, `anthropic`, `OpenAI`,
  `openai`, `GPT`, `gpt`, `Gemini`, `gemini`, `Google`, `google`, `Qwen`,
  `qwen`, `DeepSeek`, `deepseek`, `CodeLlama`, `codellama`, `StarCoder`,
  `starcoder`, `CodeWhisperer`, `codewhisperer`, and `VS Code`.
- Rewrite those references as `the hosted coding agent`, `the profile-pinned
  model`, `the proprietary reference`, `the automated reviewer`, or `the
  hosted runtime`.
- Prefer explicit state over stringly typed control flow.
- Avoid `unwrap`/`expect` in runtime paths unless construction-proven.
- Reuse existing helpers. Avoid speculative refactors.
- Every new dependency must be MIT or Apache 2.0 licensed.
- When behavior changes, add or update focused tests.
- Keep the model pinned in the profile rather than adding invocation flags.
- If `rg` is unavailable in the hosted runner, fall back to `git grep -n`,
  `grep -RIn`, or direct file reads and continue.
- If validation fails only because the hosted runner lacks a local tool that
  is not provisioned by this repository (e.g. `taplo`, `rg`), report the
  environment gap instead of installing it. Use the lighter validation set.
- Do not describe implementation work as landed unless the remote branch has
  a code-bearing commit and a visible file diff.

## Before committing

Run these commands and only commit if they pass:

```bash
cargo fmt --check
cargo test --all-targets
bash scripts/check_forbidden_names.sh
```

Run `make gate-fast` only when `taplo` and the rest of the gate tooling are
already installed in the runner image.

## Post-session workflow

- List and tail hosted sessions with the unique session identifier:

```bash
gh agent-task list
gh agent-task view <session-id-or-pr> --log --follow
```

- Inspect the hosted PR and watch its checks with:

```bash
gh pr view <pr> --json headRefName,commits,statusCheckRollup
gh pr checks <pr> --watch
```

- Open at most one draft PR for the lane. If the host creates a non-coder
  branch slug, report the session id, PR number, head branch, and any
  code-bearing commit SHAs, then stop after the draft is ready so the
  dispatcher can promote the work onto `coder/vexcoder-...`.
- If the hosted PR has only a planning commit or no file diff, report that no
  code was published and do not present the change as implemented.
- Expect the dispatcher to cherry-pick only code-bearing commits onto a
  `coder/vexcoder-...` branch, run
  `vexdraft/scripts/commit-debug.py` on the configured 2.5 review lane, patch
  findings, minimize automated review comments after fixes, sanitize PR text
  and comments, watch CI, and refresh documentation plus the raw URL map
  before merge.

## Pull requests

Use five sections: Summary, Motivation, Approach, Validation, Risks.
