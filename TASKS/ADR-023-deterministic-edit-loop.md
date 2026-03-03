# ADR-023: Deterministic edit loop — context assembly, patch-apply-validate cycle, run/test feedback, and semantic commands

**Date:** 2026-03-03
**Status:** Locked
**Deciders:** Core maintainer
**ADR chain:** ADR-020 (L7 enrichment), ADR-022 (Phase 2 command runner, Phase 3 diff-native writes), ADR-016 (tool loop guard), ADR-006 (runtime mode contracts)
**Depends on:** CRIT-19 (`PendingPatch` two-step write path), FEAT-17 (command runner one-shot), CORE-16 (approval wiring)

> **Source-verified 2026-03-03 against:** `src/app.rs` (1 712 lines), `src/runtime/task_state.rs`. See inline `[source:]` annotations for traceability. Files not fetchable in offline review are listed in the verification note at the foot of each relevant section.

---

## Context

ADR-020 established correct multi-tool round completeness, `ToolStatus::Error`, and the L7 enrichment target: an `enrich_tool_result` helper that gives the model structured, actionable context after every individual tool call rather than raw strings. L7 enrichment solves the *response quality* problem at the per-call level.

What L7 does not address is the *task-level* problem: there is no runtime construct that drives a coding task from an instruction through to a validated, committed outcome. Today a user types a request, the model proposes edits, tools execute, and the conversation continues — but every retry decision, context-gathering step, and validation call is either performed manually by the user or left to unguided model behaviour. This produces the following observable gaps:

1. The model routinely re-reads files it has already seen because no mechanism tracks what context has been assembled for the current task.
2. After a failed patch apply or a failing test run, the model receives no structured summary of what went wrong; it either loops vacuously or stalls.
3. There is no bounded stop condition for a coding task; a runaway loop can exhaust the full context window without making progress.
4. The TUI has no semantic entry points for common coding workflows; users type free-form instructions for operations that have well-defined shapes.
5. Free/open models (Qwen2.5-Coder, DeepSeek-Coder-V2, Code Llama, StarCoder2) respond significantly better to coding-specific system prompt framing and low-temperature presets than to the existing general-purpose prompt. There is currently no mechanism to load model-specific parameter profiles.

These gaps are distinct from the approval, sandboxing, and config-layering concerns addressed in ADR-022. They belong to a single coherent capability: a **deterministic edit loop** that automates the instruction → context → patch → apply → validate → retry-with-error cycle, together with the prompt and parameter infrastructure that makes free/open models perform reliably within it.

The existing runtime infrastructure makes this tractable without architectural changes:

- `RuntimeContext::start_turn` is the sole dispatch path after REF-05 (ADR-006). `[source: src/app.rs:512 ctx.start_turn(input)]`
- `ToolOperator::propose_patch` and `apply_patch` (CRIT-19) provide the two-step diff-native write gate.
- `CommandRunner` (FEAT-17, ADR-022 Phase 2) provides one-shot execution with captured stdout/stderr.
- ADR-016's tool loop guard already caps raw tool-call depth at the conversation layer; the edit loop operates one level above this, at the task level.
- ADR-020 L7's `enrich_tool_result` provides per-tool structured output; the edit loop consumes this as its retry context.

---

## Decision

Introduce six new modules as a thin, additive layer over the existing runtime. None of these modules modify the runtime core, introduce new `RuntimeMode` implementations, or add alternate routing paths.

---

### 1. Prompt templates

New directory: `src/prompts/`

```
src/prompts/coder_system.txt
src/prompts/edit_template.txt
src/prompts/explain_template.txt
src/prompts/fix_template.txt
src/prompts/plan_template.txt
src/prompts/review_template.txt
```

`coder_system.txt` is the base coding persona injected as the supplementary system prompt when the edit loop or a semantic command is active (appended after `RuntimeCorePolicy`'s base prompt, never replacing it). It instructs the model to:

- Propose changes as unified diffs or minimal inline replacements, not full-file rewrites.
- Omit commentary prose unless explicitly requested.
- Not invent file paths or symbol names absent from the assembled context.
- Read the relevant file via `read_file` before proposing a change if uncertain.
- On patch failure, read the rejection error from the context block and propose a targeted correction.

`edit_template.txt`, `explain_template.txt`, `fix_template.txt`, `plan_template.txt`, and `review_template.txt` are task-specific user-turn templates with `{{instruction}}` and `{{context}}` substitution sites. `plan_template.txt` exposes an additional `{{scope}}` substitution site for the assembled file/symbol scope the plan will cover. `review_template.txt` additionally exposes a `{{diff_context}}` substitution site for the assembled diff payload. They are loaded via `include_str!` at compile time and rendered by `EditLoop` and the standalone semantic commands before each `start_turn` call.

Constraints:

- Template files must not contain provider names, model names, or proprietary product references. CI must include a `scripts/check_forbidden_names.sh` grep check covering `src/prompts/` and `models/`. This check is added to the dispatcher checklist as **EL-09** and must pass for every checklist item from EL-06 onward.
- The coding system prompt is only injected when the edit loop or a semantic command is active. Free-form turns use the `RuntimeCorePolicy` base prompt only.
- Templates are plain UTF-8 text files. `include_str!` keeps the binary self-contained while keeping the text separately auditable.

---

### 2. Model profiles

New directory: `models/`

```
models/qwen-coder.toml
models/deepseek-coder.toml
models/starcoder.toml
models/codellama.toml
```

Each file is a `ModelProfile` TOML document:

```toml
# models/qwen-coder.toml
name             = "qwen-coder"
system_prompt    = "src/prompts/coder_system.txt"
temperature      = 0.2
top_p            = 0.95
max_tokens       = 4096
stop_sequences   = ["\n```\n", "<|endoftext|>"]
structured_tools = true
```

The corresponding Rust type:

```rust
#[derive(Deserialize)]
pub struct ModelProfile {
    pub name: String,
    pub system_prompt: PathBuf,
    pub temperature: f32,
    pub top_p: f32,
    pub max_tokens: u32,
    pub stop_sequences: Vec<String>,
    pub structured_tools: bool,
}

impl ModelProfile {
    pub fn load(path: &Path) -> Result<Self>;
    pub fn default_for_backend(backend: ModelBackendKind) -> Self;
}
```

A profile is selected by setting `VEX_MODEL_PROFILE=models/qwen-coder.toml` or the `model_profile` key in `.vex/config.toml`. When no profile is set, `ModelProfile::default_for_backend` returns a conservative default (temperature 0.3, no stop sequences, structured tools following existing backend detection logic).

Constraints:

- `ModelProfile` is loaded and validated at startup. An invalid or missing profile path is a hard failure with a diagnostic, not a silent fallback.
- `ModelProfile` does not introduce a new runtime mode. It is consumed by `RuntimeContext` when building the API request payload; it affects request parameters only.
- The `system_prompt` field is a path to a prompt template file, not an inlined text blob.
- Profile files live in `models/` at the repo root, are committed to source control, and must not reference proprietary names.
- **`structured_tools = false` fallback:** When a profile sets `structured_tools = false`, the runtime must fall back to tagged-fallback tool call mode (the same path used by `model_protocol = "chat-compat"` backends in ADR-022). This is the correct default for models that do not reliably follow structured tool schemas. A profile must never be loaded without a well-defined tool call mode resolution; the absence of a `structured_tools` key is a hard validation failure at load time.
- **Sequencing gate:** `ModelProfile` config integration (`model_profile` TOML key, `VEX_MODEL_PROFILE` env var) is gated on ADR-022 Phase 1 (layered config) completion. EL-07 (struct and files) may proceed; EL-08 (config integration) may not.

---

### 3. `ContextAssembler` — automatic pre-turn context injection

New file: `src/runtime/context_assembler.rs`

```rust
pub struct AssembledContext {
    pub file_snapshots: Vec<FileSnapshot>,
    pub git_status_summary: Option<String>,  // None in non-git repos or on timeout
    pub recent_diff: Option<String>,         // None in non-git repos or on timeout
    pub related_paths: Vec<PathBuf>,
}

pub struct FileSnapshot {
    pub path: PathBuf,
    pub content: Option<String>,  // None if unreadable; annotation included in render
    pub truncated: bool,
}

pub struct ContextAssembler {
    pub max_file_bytes: usize,   // default: 32_768
    pub max_diff_lines: usize,   // default: 200
    pub max_related: usize,      // default: 3
    pub git_timeout_ms: u64,     // default: 2_000; override via VEX_CONTEXT_GIT_TIMEOUT_MS
                                 // if the env var is present but not a valid u64, fall back to
                                 // the default and emit a startup warning — not a hard failure
}

impl ContextAssembler {
    pub fn assemble(&self, instruction: &str, operator: &ToolOperator) -> Result<AssembledContext>;
    pub fn render(&self, ctx: &AssembledContext) -> String;
}
```

`assemble` is a synchronous function with one implementation caveat: the two git subprocess calls must not be allowed to block the async runtime indefinitely. The correct implementation is to spawn each git call as a `std::process::Command` child inside a `tokio::task::spawn_blocking` closure, and wrap the resulting `JoinHandle` with `tokio::time::timeout(git_timeout_ms)`. If the timeout fires, the future is dropped — but dropping the `JoinHandle` does **not** terminate the child process; the `spawn_blocking` thread continues running until the blocking closure returns naturally. To achieve actual process termination on timeout the implementation must call `child.kill()` explicitly, either via a kill-on-drop guard struct wrapping the `Child`, or by checking a `tokio::sync::watch` receiver inside the blocking closure after each call. In either case, the corresponding `AssembledContext` field is set to `None` on timeout and a `[context: git diff timed out after 2000ms]` annotation is included in the rendered context block. This means `assemble` itself must be called from an async context via `tokio::task::block_in_place` or from within a `spawn_blocking` wrapper at the call site in `EditLoop`. Callers must not invoke `assemble` on the async executor thread directly. `[source: verify src/runtime/loop.rs for the established spawn_blocking pattern before implementing]`

After reading named files and running git metadata collection, `assemble` infers related paths from simple `use`/`import` pattern matching in named files. No embeddings, no language-server calls, no symbol index.

**Non-git repos:** If the working directory contains no `.git` ancestor, `git_status_summary` and `recent_diff` are set to `None`. Assembly continues normally; no error or warning is emitted.

**Large repos / slow git:** Both git subprocess calls are bounded by `git_timeout_ms` (default: 2 000 ms). If either call exceeds the timeout, the corresponding field is set to `None` and a `[context: git diff timed out after 2000ms]` annotation is included in the rendered context block. `git_timeout_ms` is a runtime-internal tuning parameter; it is not exposed in `ModelProfile`.

`render` serialises the `AssembledContext` into the structured context block prepended to the user turn before `start_turn` is called.

Constraints:

- All path resolution must use `ToolOperator`'s existing workspace-root confinement and lexical normalisation guards (ADR-002). `[source: verify src/tools/operator.rs before implementing]`
- If a named file exceeds `max_file_bytes`, a truncation annotation is included; the file is not silently dropped.
- `ContextAssembler` must not perform network requests.
- `ContextAssembler::assemble` must not call `ctx.start_turn()`.

---

### 4. `EditLoop` — bounded task-level runtime construct

New file: `src/runtime/edit_loop.rs`

```rust
pub struct EditLoop {
    pub task_id: TaskId,
    pub max_turns: u8,                 // default: 6; hard ceiling: 12
    pub stop_on_clean_validate: bool,
    pub profile: ModelProfile,
    last_validation_result: Option<ValidationResult>,  // in-session only; not persisted
}

pub enum EditLoopOutcome {
    Success { patch_applied: bool, validate_passed: bool },
    MaxTurnsReached { last_error: Option<String> },
    ApprovalDenied,
    Cancelled,
}

impl EditLoop {
    pub async fn run(
        &self,
        instruction: String,
        ctx: &mut RuntimeContext,
    ) -> Result<EditLoopOutcome>;

    /// Returns the last ValidationResult from this loop's most recent run.
    /// Used by /fix to pre-populate its instruction without reading TaskState.
    pub fn last_validation_result(&self) -> Option<&ValidationResult>;
}
```

**`last_validation_result` design note:** `TaskState` (the persisted struct at `src/runtime/task_state.rs`) carries no `last_error` field. `[source: task_state.rs — TaskState fields confirmed as: id, status, active_grants, changed_files, command_history, conversation_snapshot, interrupted_sessions]`. Therefore `/fix`'s pre-population of its instruction from "the last failing result" is sourced from `EditLoop::last_validation_result()`, which is in-session state held on the `EditLoop` instance in `TuiMode`. It is not persisted to disk. If `/fix` is invoked with no prior `EditLoop` run in the current session, it emits `[no recent validation failure in this session — run /edit or /test first]` and does not start a loop.

**Loop body per turn:**

```
┌─────────────────────────────────────────────────────────────────┐
│  EditLoop::run(instruction)                                     │
│                                                                 │
│  ┌─ turn N (max_turns = 6 default, ceiling 12) ───────────────┐│
│  │ 1. workspace_dirty_check()  ──── dirty? → warn, continue  ││
│  │ 2. ContextAssembler::assemble(instruction)                 ││
│  │ 3. render edit_template.txt {instruction, context}         ││
│  │ 4. inject coder_system.txt as supplementary system prompt  ││
│  │ 5. ctx.start_turn(rendered)  ◄── sole dispatch path        ││
│  │       ↓ TurnComplete + tool results (L7 enriched)          ││
│  │ 6. PendingPatch produced?                                  ││
│  │       yes → CORE-16 approval gate                          ││
│  │               deny  → return ApprovalDenied                ││
│  │               approve → apply_patch → ValidationSuite::run ││
│  │                   pass  → return Success                   ││
│  │                   fail  → store in last_validation_result  ││
│  │       no patch → no validation                             ││
│  │ 7. turn_counter++                                          ││
│  │       == max_turns → return MaxTurnsReached{last_error}    ││
│  │       else → enrich retry ctx, goto 1                      ││
│  └────────────────────────────────────────────────────────────┘│
│  CancellationToken checked at every await (steps 5, 6)         │
│  → Cancelled returned immediately; no state mutation after     │
└─────────────────────────────────────────────────────────────────┘
```

**Workspace-dirty check:** Before the first `apply_patch` in a run, `EditLoop` checks `git status --porcelain` for uncommitted changes to any file present in `AssembledContext::file_snapshots`. If dirty files are found, a structured warning is emitted to the TUI transcript via `push_history_line`. The loop does not stash, commit, or otherwise mutate git state autonomously. The operator may proceed or cancel. `[source: src/app.rs:310 push_history_line pattern]`

**Multi-language `ValidationSuite` inference:** `ValidationSuite::infer_from_repo` detects project shape and builds a command list. When both `Cargo.toml` and `package.json` are present at the repo root, both `cargo check && cargo test` and `npm test` are added to the suite in that order. Additional `Makefile` targets are appended if a `test` target exists. When no recognisable project files are found, the suite is empty and the loop terminates on first clean patch application.

`EditLoop::run` is `async`. `CancellationToken` propagation follows the pattern established in `src/runtime/loop.rs` and must be checked at every `await` point. A fired token returns `EditLoopOutcome::Cancelled` immediately; no state is mutated after the token fires. `[source: verify src/runtime/loop.rs CancellationToken pattern before implementing]`

Constraints:

- `EditLoop` must not hold a reference to any TUI type (`ratatui`, `crossterm`).
- `EditLoop` must not call `ToolOperator` directly. File mutations flow through the conversation tool dispatch layer only.
- `EditLoop` must not exceed `max_turns` under any circumstance. The counter is checked *before* `ctx.start_turn()`, not after.
- **Reentrancy is prohibited.** `TuiMode::try_handle_slash_command` must check whether `active_edit_loop` is already `Some` before spawning a new loop. If a loop is in progress, the `/edit` and `/fix` commands must emit `[edit loop already active — cancel with Ctrl+C before starting a new task]` and return without spawning. Nested or concurrent loop invocations are not supported and must not be possible through any code path.
- On `MaxTurnsReached`, the loop outcome and last error string are surfaced to the operator via `push_history_line` in the TUI transcript. `TaskState::command_history` records the `CommandEvidence` entries (program, exit code, interrupted flag) for each validation command that ran — it does not hold a structured error string, and the loop must not attempt to write one there. `[source: task_state.rs CommandEvidence struct — fields: program, exit_code, interrupted]`. If the operator wants to resume, they use `/fix` in the same session (sourced from `last_validation_result`) or `vex exec` with the task file and JSONL output for offline inspection.
- `EditLoop` is not a `RuntimeMode`.

---

### 5. `ValidationSuite` — post-apply feedback for retry enrichment

New file: `src/runtime/validation.rs`

```rust
pub struct ValidationSuite {
    pub commands: Vec<ValidationCommand>,
}

pub struct ValidationCommand {
    pub label: String,      // e.g. "cargo check", "npm test"
    pub args: Vec<String>,
    pub timeout_secs: u64,  // default: 60
}

pub struct ValidationResult {
    pub passed: bool,
    pub outputs: Vec<ValidationOutput>,
}

pub struct ValidationOutput {
    pub label: String,
    pub exit_code: i32,
    pub stdout_tail: String,  // capped at VALIDATION_TAIL_BYTES (default: 8_192)
    pub stderr_tail: String,  // capped at VALIDATION_TAIL_BYTES (default: 8_192)
}

impl ValidationSuite {
    pub async fn run(&self, runner: &dyn CommandRunner) -> Result<ValidationResult>;
    pub fn format_for_retry(&self, result: &ValidationResult) -> String;
    pub fn infer_from_repo(root: &Path) -> Self;
    pub fn load_or_infer(root: &Path) -> Self;  // checks .vex/validate.toml first
}
```

`format_for_retry` produces the structured error block injected into the next turn at loop step 7. It closes the ADR-020 L7 feedback loop at the task level.

`stdout_tail` and `stderr_tail` are truncated to `VALIDATION_TAIL_BYTES` (default: 8 192 bytes) from the end of each stream. This keeps `last_validation_result` in-session memory bounded regardless of how verbose a test runner is. Truncation is applied during `ValidationSuite::run` before the output is stored, not at render time. The truncation boundary must be annotated in `format_for_retry` output: `[stdout truncated — showing last 8192 bytes]`.

`load_or_infer` loads from `.vex/validate.toml` if present, falling back to `infer_from_repo`. The TOML shape mirrors `ValidationCommand` fields directly.

Constraints:

- `ValidationSuite::run` must delegate to `CommandRunner::run_one_shot` (FEAT-17). Direct `std::process::Command` in `validation.rs` is prohibited.
- `ValidationSuite` must not mutate any file.
- **`ValidationSuite` itself has no patch precondition.** The constraint that validation only runs after a patch apply is an `EditLoop`-level policy (loop step 6): within the loop, `ValidationSuite::run` is only called when `apply_patch` has succeeded in the current turn. Standalone invocations — `/run`, `/test`, and `/review` — call `ValidationSuite::run` directly with no patch precondition; this is intentional and correct. The two uses are distinct: loop-internal validation feeds the retry context; standalone validation feeds the operator's transcript. Do not add a patch-check guard to `ValidationSuite::run` itself.

---

### 6. Semantic slash commands — `/edit`, `/fix`, `/explain`, `/run`, `/test`, `/review`, `/plan`, `/context`

**Source-critical note:** `TuiMode::on_user_input` in `src/app.rs` contains no slash-command dispatch. `[source: app.rs:484–512 — on_user_input calls ctx.start_turn(input) unconditionally after overlay and busy-guard checks]`. This ADR introduces the dispatch branch as new code. There is no existing dispatch to wire against.

The dispatch is introduced at the top of `on_user_input` after the overlay and busy-guard checks, before `ctx.start_turn`:

```rust
// src/app.rs — within TuiMode::on_user_input, after busy guard, before start_turn
if let Some(outcome) = self.try_handle_slash_command(&input, ctx) {
    self.push_history_line(format!("> {input}"));
    self.push_history_line(String::new());
    // slash commands that do not start a turn skip the turn_in_progress flag
    match outcome {
        SlashCommandOutcome::Handled => {}
        SlashCommandOutcome::StartTurn(rendered) => {
            self.history_state.active_assistant_index =
                Some(self.history_state.lines.len() - 1);
            self.history_state.turn_in_progress = true;
            ctx.start_turn(rendered);
        }
    }
    return;
}
```

`try_handle_slash_command` returns `None` for non-slash input, falling through to the existing `ctx.start_turn(input)` path unchanged.

`TuiMode` gains one new field:

```rust
// src/app.rs — TuiMode struct
active_edit_loop: Option<EditLoop>,  // carries last_validation_result between /edit and /fix
```

**Edit-loop commands** (invoke `EditLoop::run` via a spawned async task, reporting outcome via the existing `UiUpdate` channel):

```
/edit <instruction>
    Starts an EditLoop with stop_on_clean_validate=true.
    Renders edit_template.txt with the provided instruction and assembled context.

/fix
    Sources instruction from active_edit_loop.last_validation_result().
    If None → push_history_line("[no recent validation failure in this session
    — run /edit or /test first]") and return.
    Starts an EditLoop with fix_template.txt pre-populated from the last result.
```

**Read-only commands** (no `EditLoop`; single turn or no model turn):

```
/explain [path]
    Assembles context for the named path (or most recently accessed file if none
    given) and starts a single ctx.start_turn using explain_template.txt.
    No patch proposal is accepted. EditLoop is not invoked.

/run [command]
    Invokes ValidationSuite with a single user-specified command (or the first
    inferred command if none given). Renders output to transcript via
    push_history_line. Does not start an EditLoop or a model turn.

/test
    Equivalent to /run with the full inferred or configured ValidationSuite.
    Renders all outputs to transcript. Does not start an EditLoop.

/plan <instruction>
    Assembles context for the files named in <instruction> (same path as
    ContextAssembler::assemble) and starts a single ctx.start_turn using
    plan_template.txt. EditLoop is not invoked. Any PendingPatch the model
    produces is silently dropped — /plan is read-only. The model is instructed
    by plan_template.txt to output a numbered plan with file paths and change
    descriptions only; it must not emit tool calls or diffs.

    plan_template.txt substitution sites:
      {{instruction}}   The operator-supplied planning instruction.
      {{scope}}         The assembled file-snapshot block (paths + content).
      {{context}}       The git_status_summary if available; empty string otherwise.

    Typical use: operator runs /plan, reads the plan, then issues /edit with
    the same instruction to execute it. /plan output is never automatically
    forwarded to /edit; the operator decides whether to proceed.

/context
    Renders current session state to the TUI transcript via push_history_line.
    No model turn is started. Output format (normative):

      [context]
        model     : <active model name>
        backend   : <ModelBackendKind>
        profile   : <ModelProfile name or "default">
        task      : <TaskId>
        status    : <TaskStatus>
        turns     : <EditLoop turn counter if active, else "—">
        files     : <count of file snapshots in most recent AssembledContext>
        git       : <git_status_summary first line, or "no git" / "timed out">
        approvals : <active_grants count> active grant(s)
        tokens    : ~<estimated token count for conversation so far>

    Token estimation uses the same chars ÷ 4 approximation used elsewhere in
    this ADR chain. The annotation "~" is mandatory — the estimate is not exact.
    /context is the operator-visible counterpart to the formally-deferred
    compaction gap (ADR-024 §deferred); it surfaces the information an operator
    needs to decide whether to start a new session.
```

`/run` and `/test` make the validation infrastructure independently accessible outside the edit loop. `/explain` is a read-only, no-patch workflow that uses the coding system prompt but creates no `PendingPatch`. `/review` is a read-only diff-analysis workflow; it assembles diff or file context and starts a single model turn with no patch gate. `/plan` is a read-only planning workflow; it produces a numbered plan without executing any changes. `/context` is a zero-turn status command; it renders session state to the transcript without starting a model turn.

**Review command** (no `EditLoop`; single model turn; no patch accepted):

```
/review [--base <git-ref>] [--files <glob>] [<instruction>]

    Assembles a diff context and starts a single ctx.start_turn using
    review_template.txt. EditLoop is not invoked. Any PendingPatch the model
    produces is silently dropped — /review is read-only and never calls
    apply_patch.

    Data-source resolution (normative):
      --base <git-ref>   Diff assembled from `git diff <git-ref>` against the
                         working tree. <git-ref> may be any ref git accepts:
                         a commit SHA, branch name, or symbolic ref (e.g. HEAD~1,
                         main, origin/main). Validated by `git rev-parse
                         --verify <git-ref>` at parse time; invalid ref emits
                         "[review: invalid base ref '<git-ref>']" and returns
                         without starting a turn.
      --files <glob>     ContextAssembler::assemble is called for all paths
                         matching <glob> relative to the workspace root.
                         Assembled context (file snapshots + git_status_summary)
                         is used in place of a diff. Incompatible with --base;
                         providing both emits "[review: --base and --files are
                         mutually exclusive]" and returns.
      (neither flag)     Equivalent to --base HEAD: assembles `git diff HEAD`
                         (staged + unstaged changes in the working tree).
      <instruction>      Optional free-text appended to the {{instruction}}
                         substitution site in review_template.txt. If absent,
                         review_template.txt provides a default instruction
                         ("Review these changes for correctness, clarity, and
                         potential issues.").

    review_template.txt substitution sites:
      {{diff_context}}   The assembled diff or file-snapshot block.
      {{instruction}}    The operator-supplied instruction or the default.
      {{context}}        Populated only when --files is used; empty string
                         otherwise.
```

`/review` uses `ContextAssembler`'s existing `assemble` path for `--files` mode and calls `git diff <ref>` directly via the same `spawn_blocking` + `git_timeout_ms` mechanism as the `recent_diff` field, for `--base` mode. No new subprocess mechanism is introduced.

All eight commands surface their outcome as a structured message in the TUI transcript on completion and do not replace free-form prompting.

---

## Rationale

### Why is `last_validation_result` in-session state on `EditLoop`, not persisted in `TaskState`?

`TaskState` is the persisted, disk-backed struct. Its current schema contains no error payload field `[source: task_state.rs fields confirmed]`. Adding `last_error: Option<String>` to the persisted struct would require a migration path for existing state files and would tie the error payload lifetime to task persistence — longer than needed. The error is only meaningful in the current session, and only until the next successful validation. Storing it on the in-session `EditLoop` instance makes the lifetime explicit and avoids both schema evolution and a migration.

### Why is slash-command dispatch added as a new branch in `on_user_input`, not as a new `RuntimeMode`?

`on_user_input` already guards against overlays and busy state before dispatching. Slash commands need the same guards. Adding a `try_handle_slash_command` branch before the unconditional `ctx.start_turn(input)` path is the minimum change: it intercepts the eight new commands, falls through to `start_turn` for everything else, and does not require touching `RuntimeMode`, `on_frontend_event`, or any other path. A new `RuntimeMode` for slash commands would duplicate the overlay and busy guards already proven in `TuiMode`. `[source: app.rs:484–512 on_user_input structure]`

### Why a separate `EditLoop` rather than extending `Runtime<M>`?

The main loop (ADR-006) is a generic event driver. Embedding task-level state (turn counters, patch tracking, validation results) in it would couple task semantics to event routing and make the loop significantly harder to test. `EditLoop` is a named, bounded, independently testable construct invoked by the mode when needed.

### Why is `ContextAssembler::assemble` synchronous at its call signature?

The file-read and pattern-matching portions of context assembly are purely local, fast, and non-async. The git subprocess calls are the only latency risk and they are handled via `tokio::task::spawn_blocking` + `tokio::time::timeout` so they do not block the async executor. Keeping `assemble`'s signature synchronous simplifies the `AssembledContext` value-return shape and avoids requiring callers to `await` a function that is mostly CPU-bound local work. The caller (`EditLoop`) invokes `assemble` inside a `spawn_blocking` wrapper at step 1 of the loop body, which is the correct integration point.

### Why do `ContextAssembler`'s git calls use `std::process::Command` rather than `CommandRunner`?

History visibility. These calls are pre-turn and must not appear as tool calls in the conversation history or the approval flow. `ValidationSuite` runs post-patch and its outputs feed back into the model turn; routing through `CommandRunner` keeps those outputs structured, timeout-guarded, and consistent with the evidence model. The two mechanisms serve distinct purposes and must not be conflated.

### Why cap `max_turns` at 12?

ADR-016 caps raw tool-call depth at the conversation layer; the edit loop operates one level above. Both caps apply simultaneously. Six turns covers the majority of single-file edits with one or two validate-retry cycles. A ceiling of 12 prevents context-window exhaustion on free/open models with 8k–16k context windows.

### Why does `/plan` not start an `EditLoop` or accept patches?

`/plan` is the deliberation step that precedes `/edit`. Its value is producing a human-readable plan the operator can verify and modify before any file is changed. Starting an `EditLoop` would bypass that gate. Silently dropping any `PendingPatch` the model produces keeps `/plan` safe to run at any point — including on a dirty working tree — without risk of unintended writes. The operator-confirms-then-executes pattern is the correct design for a planning command: produce the plan, let the operator decide, then execute only on explicit instruction.

### Why does `/context` start no model turn?

`/context` is a pure inspection command. Its output is computed locally from `TuiMode` state, `TaskState`, and the most recently cached `AssembledContext`; no model inference is required or appropriate. Starting a turn for a status query would consume context window budget unnecessarily and add latency. The token estimate it surfaces is the primary signal operators need to decide whether to start a new session — making it a zero-turn command means it remains usable even when the context window is nearly exhausted.

### Why does `/review` not start an `EditLoop`?

`/review` is a read-only analytical command. Its intent is to surface feedback on existing changes, not to propose or apply new ones. Starting an `EditLoop` would introduce a patch-apply-validate cycle for a command whose correct outcome is a model-generated commentary turn. The read-only boundary also means `/review` is always safe to run mid-session without affecting working-tree state.

### Why does `/review` drop any `PendingPatch` the model produces?

The model cannot be prevented at the API level from proposing tool calls during any turn. `/review` must silently discard any `PendingPatch` rather than surfacing an approval prompt, because surfacing an approval would confuse operators who issued a read-only command. The drop is silent to the operator; a debug-level log entry is sufficient. This is the same posture as `/explain` — no patch from a read-only command is ever approved.

### Why are `/run` and `/test` not starting an `EditLoop`?

The validation infrastructure is useful independently of model turns. Making these commands independent of `EditLoop` keeps them fast, predictable, and safe to run at any point in a session.

### Why does `/fix` not fall back to `TaskState::command_history`?

`command_history` records `CommandEvidence` (program name, exit code, interrupted flag) — not structured validation output. `[source: task_state.rs CommandEvidence struct]`. Reconstructing a useful error context from those fields would require heuristics. The `ValidationResult` held in `last_validation_result` already contains `stdout_tail`, `stderr_tail`, and `passed`, which is exactly what `fix_template.txt` needs.

---

## Alternatives considered

### Implement `EditLoop` as a new `RuntimeMode`

Rejected. A dedicated `EditMode: RuntimeMode` would require duplicating or proxying the TUI render and input handling already in `TuiMode`. The edit loop is a sub-behaviour of an interactive session, not a standalone mode.

### Load `ValidationSuite` entirely from config with no inference

Rejected. Purely config-driven validation requires operators to create `.vex/validate.toml` before the loop is useful. For common project shapes (Rust, Node, Make), inference makes the loop work out of the box.

### Inline prompt text as `&'static str` constants

Rejected. Inline constants are harder to audit for prohibited names, cannot be diffed without reading Rust source, and discourage iterative prompt improvement.

### Use `RuntimeCorePolicy` to inject the coding system prompt

Rejected. `RuntimeCorePolicy` applies globally. The coding prompt is narrowly scoped to active edit-loop and semantic-command turns.

### Embedding-based context assembly

Rejected. Requires a vector database and an embedding model. Simple `use`/`import` pattern matching provides acceptable related-file inference with no dependencies.

### Persist `last_error` to `TaskState`

Rejected. `TaskState`'s `CommandEvidence` struct records execution facts, not structured validation payloads. Adding a `last_error` field to the persisted struct introduces a schema migration obligation for a value whose useful lifetime is bounded to the current session. In-session state on `EditLoop` is sufficient.

---

## Consequences

**Easier:**

- Coding tasks that previously required manual retry-and-re-prompt cycles are automated within a bounded, auditable loop.
- Free/open models see structured context and error feedback instead of raw conversation history, improving patch quality without fine-tuning.
- The TUI gains eight high-value entry points that make the agent's core capabilities immediately discoverable.
- `ValidationSuite` is independently testable with `CommandRunner` mocks from ADR-005's injection pattern.
- Model parameter tuning for free/open models is encapsulated in committed, auditable TOML files.

**Harder:**

- `EditLoop` adds async task-level state that must be cancelled cleanly on interrupt. `CancellationToken` propagation must be verified at every `await` point.
- The new `active_edit_loop: Option<EditLoop>` field on `TuiMode` must be cleared correctly on session reset.
- The interaction between `max_turns` (task level) and ADR-016's per-turn tool-call depth cap (conversation level) must be documented clearly in `src/runtime/edit_loop.rs`; both limits apply simultaneously.

**Known limitations:**

- `ContextAssembler` infers related files from `use`/`import` pattern matching only. Repos with non-standard module structures (C, unconventional Rust workspace layouts) may see incomplete related-path inference. Operators can supplement by naming additional files explicitly in their instruction.
- `ValidationSuite::infer_from_repo` assumes `Cargo.toml`, `package.json`, or `Makefile` at the repo root. Makefile `test` target detection uses `grep -E "^test:" Makefile` — non-standard target names (e.g. `check`, `tests`) will not be detected and require `.vex/validate.toml`. Monorepos with nested project roots must also provide `.vex/validate.toml` for correct inference.
- **Within `EditLoop`:** loop turns that produce tool calls but no `PendingPatch` do not run validation. Validation in the loop requires a patch apply to have occurred. If a model turn reads files, runs commands, and produces output without proposing a diff, the loop increments the turn counter and retries. Operators who observe a loop consuming turns without patching should verify the instruction includes an explicit edit directive. This constraint is `EditLoop`-scoped; standalone `/run`, `/test`, and `/review` are not subject to it.
- **Interrupt between apply and validate:** If `CancellationToken` fires after `apply_patch` completes but before `ValidationSuite::run` returns, the patch is already written to disk. `EditLoopOutcome::Cancelled` is returned and the working tree contains the applied patch. This is not automatically rolled back. The operator's working tree is in a modified state and they must inspect and revert manually if needed. This window is noted as an inherent consequence of the two-step write gate in CRIT-19 and is not addressable without a rollback mechanism that is out of scope for this ADR.
- In-session `/fix` pre-population is lost if the process is restarted between the failed run and the `/fix` invocation. Operators with long-running sessions who need persistent error context should use `vex exec --task-file` and inspect the JSONL output.
- Git operations in `ContextAssembler` are bounded by `git_timeout_ms` (default: 2 000 ms) via `tokio::task::spawn_blocking` + `tokio::time::timeout`. Very large repos with slow I/O may see diff context degraded to `None` on first run; the timeout is configurable via `VEX_CONTEXT_GIT_TIMEOUT_MS`.

**Documentation updates required (must accompany EL-06):**

- `docs/src/generated/tools.md` — add `/edit`, `/fix`, `/explain`, `/run`, `/test`, `/review`, `/plan`, `/context` to the command reference.
- `docs/src/policy.md` — add `Capability::ApplyPatch` approval note for edit-loop context.

**Constraints imposed on future work:**

- `EditLoop` must not call `ctx.start_turn()` more than `max_turns` times. Any future extension adding turns must decrement from the same counter.
- `ContextAssembler` path resolution must use `ToolOperator`'s workspace-root guards. No second implementation is permitted.
- All files in `src/prompts/` and `models/` must not contain provider names. `scripts/check_forbidden_names.sh` must cover both directories.
- `ValidationSuite::run` must route through `CommandRunner`. Direct `std::process::Command` in `validation.rs` is prohibited.
- `ModelProfile` config integration (EL-08) must not be implemented until ADR-022 Phase 1 is complete.
- No `ratatui` or `crossterm` imports in `src/runtime/edit_loop.rs`, `context_assembler.rs`, or `validation.rs`.
- `active_edit_loop` on `TuiMode` must be cleared when a session ends or is reset.

---

## Dispatcher checklist

| ID | Task | Gate | Status |
| :--- | :--- | :--- | :--- |
| **EL-01** | `ContextAssembler` stub — named path snapshots, git summary, git timeout | Must be green before EL-03 | [ ] |
| **EL-02** | `ValidationSuite` — `CommandRunner` mock, `format_for_retry`, multi-lang inference | Must be green before EL-03 | [ ] |
| **EL-03** | `EditLoop::run` skeleton — `max_turns` guard, `Cancelled` outcome, workspace-dirty check | Must be green before EL-04 | [ ] |
| **EL-04** | `/edit` and `/fix` wired via `try_handle_slash_command`; `active_edit_loop` field on `TuiMode`; `/fix` no-prior-result guard | Must be green before EL-05 | [ ] |
| **EL-05** | `/explain`, `/run`, `/test` — no `EditLoop` invocation (note: `/review` is introduced separately in EL-10) | Must be green before EL-06 | [ ] |
| **EL-06** | `src/prompts/` templates; `coder_system.txt` injection on loop activation only; `docs/` updates | Must be green before EL-07 | [ ] |
| **EL-07** | `ModelProfile` struct; `models/*.toml` files; `default_for_backend` fallback | Must be green; EL-08 gated separately | [ ] |
| **EL-08** | `ModelProfile` config integration via layered config | **Gated: ADR-022 Phase 1 must be complete** | [ ] |
| **EL-09** | `scripts/check_forbidden_names.sh` — covers `src/prompts/` and `models/`; added to CI | Must pass for all items EL-06 onward | [ ] |
| **EL-10** | `/review` — `review_template.txt`; `--base`/`--files` flag parsing; ref validation; PendingPatch drop; diff assembly via `spawn_blocking` | Must be green after EL-05; gated on EL-06 for template | [ ] |
| **EL-11** | `/plan` — `plan_template.txt`; `{{scope}}` assembly via `ContextAssembler`; PendingPatch drop; no EditLoop invocation | Must be green after EL-05; gated on EL-06 for template | [ ] |
| **EL-12** | `/context` — zero-turn status render; token estimate; `active_grants` count; git summary; no model turn | Must be green after EL-05 | [ ] |
| **EL-13** | `/commands` and `/help` alias — runtime-generated from dispatch table; description registration; compile-error for missing descriptions | Must be green after EL-04 | [ ] |

## Dispatcher reporting contract (mandatory per checklist item)

When checking a box above, append an evidence block under this section:

```markdown
### [EL-01 … EL-13] - <short title>
- Dispatcher: <name/id>
- Commit: <sha>
- Files changed:
  - `path/to/file.rs` (+<insertions> -<deletions>)
- Line references:
  - `path/to/file.rs:<line>`
- Validation:
  - `cargo test --all-targets` : pass/fail
  - `check_no_alternate_routing.sh` : pass/fail
  - `check_forbidden_imports.sh` : pass/fail
  - `check_forbidden_names.sh` : pass/fail (required from EL-06)
- Notes:
  - <what was built and why>
```

---

## Task sequence and anchor tests

| Task | Scope | Anchor test | Gate |
| :--- | :--- | :--- | :--- |
| EL-01 | `ContextAssembler` — named path snapshots, git summary, non-git fallback, timeout | `test_context_assembler_includes_named_file_snapshot`; `test_context_assembler_non_git_repo_returns_none_diff`; `test_context_assembler_git_timeout_returns_none_with_annotation` | Must be green before EL-03 |
| EL-02 | `ValidationSuite` — `CommandRunner` mock, `format_for_retry`, multi-lang inference | `test_validation_suite_formats_failure_for_retry`; `test_validation_suite_infers_rust_and_node_when_both_present` | Must be green before EL-03 |
| EL-03 | `EditLoop::run` skeleton — `max_turns` guard, `Cancelled`, workspace-dirty warning, reentrancy block | `test_edit_loop_terminates_at_max_turns`; `test_edit_loop_emits_dirty_workspace_warning`; `test_edit_loop_cancel_mid_validation`; `test_tui_second_edit_command_blocked_while_loop_active` | Must be green before EL-04 |
| EL-04 | `try_handle_slash_command`; `/edit` and `/fix` dispatch; `active_edit_loop` on `TuiMode`; `/fix` guard | `test_tui_edit_command_starts_edit_loop`; `test_tui_fix_without_prior_loop_emits_guidance`; `test_slash_command_does_not_call_start_turn_directly`; `test_slash_command_returns_none_for_non_slash_input`; `test_validation_suite_empty_suite_exits_on_clean_patch`; `test_tui_fix_during_active_edit_emits_reentrancy_guard` | Must be green before EL-05 |
| EL-05 | `/explain`, `/run`, `/test` — no `EditLoop` invocation (note: `/review` is EL-10) | `test_tui_explain_does_not_invoke_edit_loop`; `test_tui_run_command_invokes_validation_suite_only` | Must be green before EL-06 |
| EL-06 | `src/prompts/` templates; injection on loop activation only; docs updated | `test_coding_prompt_injected_during_edit_loop_only`; `test_docs_tools_md_lists_slash_commands` | Must be green before EL-07 |
| EL-07 | `ModelProfile` struct; `models/*.toml`; `default_for_backend`; `structured_tools` fallback | `test_model_profile_loads_from_toml`; `test_model_profile_invalid_path_is_hard_failure`; `test_model_profile_structured_tools_false_uses_tagged_fallback`; `test_context_assembler_invalid_git_timeout_env_falls_back_to_default` | Must be green; EL-08 gated separately |
| EL-08 | `ModelProfile` config integration via layered config | `test_model_profile_loaded_from_layered_config` | **Gated: ADR-022 Phase 1 must be complete** |
| EL-09 | `scripts/check_forbidden_names.sh` — CI coverage of `src/prompts/` and `models/` | `check_forbidden_names_sh_blocks_proprietary_name_in_prompts_dir` | Must pass for EL-06 onward |
| EL-10 | `/review` — flag parsing, ref validation error paths, PendingPatch drop, diff assembly | `test_tui_review_default_assembles_head_diff`; `test_tui_review_base_flag_validates_ref`; `test_tui_review_invalid_ref_emits_error_no_turn`; `test_tui_review_mutual_exclusion_base_and_files`; `test_tui_review_drops_pending_patch_silently`; `test_tui_review_files_flag_uses_context_assembler` | Must be green after EL-05; template requires EL-06 |
| EL-11 | `/plan` — `plan_template.txt`; `{{scope}}` via `ContextAssembler`; PendingPatch drop; no EditLoop | `test_tui_plan_starts_single_turn_no_loop`; `test_tui_plan_drops_pending_patch_silently`; `test_tui_plan_scope_populated_from_assembler` | Must be green after EL-05; template requires EL-06 |
| EL-12 | `/context` — zero-turn status output; token estimate annotation; no `ctx.start_turn` call | `test_tui_context_renders_without_model_turn`; `test_tui_context_shows_tilde_token_estimate`; `test_tui_context_shows_active_grants_count` | Must be green after EL-05 |
| EL-13 | `/commands` and `/help` — runtime-generated from dispatch table; description per entry; compile error for missing description | `test_tui_commands_renders_all_registered_commands`; `test_tui_help_is_alias_for_commands`; `test_commands_output_does_not_call_start_turn`; `test_missing_command_description_is_compile_error` | Must be green after EL-04 |

All tasks require `cargo test --all-targets`, `check_no_alternate_routing.sh`, `check_forbidden_imports.sh`, and (from EL-06) `check_forbidden_names.sh` to be green after every change. No task may touch files outside its declared scope.

---

## Compliance notes for agents

| Rule | Enforcement |
| :--- | :--- |
| Do not call `ctx.start_turn()` more than `max_turns` times within a single `EditLoop::run` invocation | Counter checked before each call, not after |
| Do not call `ToolOperator` directly from `EditLoop` or `ContextAssembler` | File mutations must flow through the conversation tool dispatch layer |
| Do not use `std::process::Command` in `src/runtime/validation.rs` | All subprocess calls must route through `CommandRunner::run_one_shot` |
| **`std::process::Command` IS permitted in `src/runtime/context_assembler.rs` for the two git read calls only** (`git status --short`, `git diff HEAD`) | These calls must not appear in the tool history or approval flow. Any other subprocess in `context_assembler.rs` is prohibited |
| Do not inject the coding system prompt outside of an active `EditLoop` or semantic command turn | Verified by `test_coding_prompt_injected_during_edit_loop_only` |
| Do not add provider names, model names, or proprietary product references to any file in `src/prompts/` or `models/` | `scripts/check_forbidden_names.sh` CI check (EL-09). The script must grep with at minimum: `grep -rniE "openai\|anthropic\|claude\|gemini\|gpt\|copilot\|cursor\|codewhisperer" src/prompts/ models/` — any match is a CI failure |
| Do not implement `EditLoop` as a `RuntimeMode` | |
| Do not implement EL-08 until ADR-022 Phase 1 is complete | |
| Do not bypass `ContextAssembler`'s path-safety checks | All file reads must use `ToolOperator`'s workspace-root confinement guards |
| `EditLoop` must propagate `CancellationToken` to every `await` point | A cancelled loop must return `EditLoopOutcome::Cancelled` and must not mutate state after the token fires |
| Do not introduce `ratatui` or `crossterm` imports in `src/runtime/edit_loop.rs`, `src/runtime/context_assembler.rs`, or `src/runtime/validation.rs` | |
| Do not source `/fix` pre-population from `TaskState` | `TaskState` carries no structured error payload. Use `EditLoop::last_validation_result()` only |
| Clear `TuiMode::active_edit_loop` on session end or reset | Stale loop state must not persist across sessions |
| Do not invoke `EditLoop::run` while `TuiMode::active_edit_loop` is already `Some` | `try_handle_slash_command` must guard against this; verified by `test_tui_second_edit_command_blocked_while_loop_active` |
| `/review` must never call `apply_patch` or surface a `PendingApproval` overlay | Any `PendingPatch` produced during a `/review` turn must be silently dropped; verified by `test_tui_review_drops_pending_patch_silently` |
| `/plan` must never call `apply_patch`, invoke `EditLoop`, or surface a `PendingApproval` overlay | Any `PendingPatch` produced during a `/plan` turn must be silently dropped; verified by `test_tui_plan_drops_pending_patch_silently` |
| `/context` must never call `ctx.start_turn` | All output must be rendered via `push_history_line` only; verified by `test_tui_context_renders_without_model_turn` |
| `/review --base <ref>` must validate `<ref>` via `git rev-parse --verify` before starting a turn | Invalid ref must emit a structured error message and return without calling `ctx.start_turn` |
| `ValidationSuite::run` has no patch precondition at the function level | The EditLoop-internal "only validate after apply" policy must live in `EditLoop` step 6, not in `ValidationSuite::run` |
| `try_handle_slash_command` must return `None` for all non-`/` input | The existing `ctx.start_turn(input)` path must be reached unchanged for free-form turns |