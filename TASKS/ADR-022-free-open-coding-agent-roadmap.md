# ADR-022: Free/Open Coding Agent Roadmap

**Date:** 2026-03-01
**Status:** Proposed
**Deciders:** Core maintainer
**ADR chain:** ADR-014, ADR-018, ADR-020, ADR-021
**Amendment:** 2026-03-03 — Decision item 1 and the final Compliance note scoped to first milestone; Decision item 11 added to reserve native packaging and editor surfaces for post-first-milestone ADRs. See ADR-024 §Gap 9 for the binary distribution and macOS packaging decision.

## Context

`vexcoder` is already a Rust terminal coding assistant, but the current product
shape is still closer to a chat-first TUI than a full coding agent.

The roadmap target is a terminal-first coding agent built only around free/open
software and no-cost, self-hostable deployment paths.

The current codebase has several material gaps relative to that target:

- the configuration layer still uses legacy provider-branded environment
  variables, branded endpoint defaults, and vendor-specific model validation
  rules in `src/config.rs`
- the model layer is still organized around a provider-shaped client abstraction
  in `src/api/client.rs`, rather than a neutral backend seam
- existing file mutation helpers in `src/tools/operator.rs` perform direct
  writes and edit-in-place replacement without a diff-native approval flow
- no general command runner exists today; the tool layer provides file and git
  helpers, but not first-class command execution with streaming output,
  cancellation, or PTY support
- durable resumable task state does not yet exist as a first-class persisted
  runtime concept
- the TUI remains primarily conversation-oriented rather than
  task-execution-oriented

Two architectural boundaries are especially important for this roadmap.

First, ADR-018 is a sequencing dependency for the TUI phase of this work. The
task-execution UI defined here depends on ADR-018's managed TUI, scrollback, and
streaming infrastructure.

Second, the existing `RuntimeCorePolicy` in `src/runtime/policy.rs` is a
prompt-shaping and evidence-shaping concern. It is not an approval system. This
ADR introduces a separate capability-gating approval model and those two policy
layers must remain distinct.

Browser automation is intentionally deferred until the core repo/file/shell
agent loop is stable.

## Decision

This ADR locks the following decisions:

1. `vexcoder` is terminal-agent-first for the first milestone. The terminal runtime is the canonical execution surface and must remain so at every packaging layer. Native application packaging (e.g. a macOS wrapper) and editor-surface integration (e.g. a VS Code extension) are not in scope for the first milestone and must not be allowed to drive architectural changes to the runtime core.
2. The default operating posture is approval-first.
3. The first milestone supports both local model runtimes and self-hosted,
   neutral-compatible model servers.
4. The first milestone is interactive and resumable.
5. Background queueing is out of scope for the first milestone.
6. Browser automation is out of scope for the first milestone.
7. Legacy provider-branded configuration names, branded defaults, and
   vendor-specific validation rules are removed immediately.
8. Existing-file mutations become diff-native and approval-gated.
9. Command execution becomes a first-class built-in capability.
10. Approval is capability-based and remains separate from `RuntimeCorePolicy`.
11. Native application packaging and additional runtime surfaces are reserved for post-first-milestone work. When introduced, they must be implemented in one of two forms: (a) a *packaging layer* — wraps the compiled binary, adds OS-native credential storage and chrome, contains no agent logic; or (b) a *new `RuntimeMode` implementation* — implements `RuntimeMode + FrontendAdapter` against the shared runtime core, lives in `src/` like `TuiMode` and `BatchMode`, and extends rather than replaces the existing dispatch architecture. A local HTTP or Unix socket API server (`LocalApiServer: RuntimeMode + FrontendAdapter`) is a canonical example of form (b): it is not a packaging layer, it is a new surface implementation, and it belongs in `src/` by design. The prohibited case is an *architectural fork*: a surface that requires changes to `src/runtime/`, `src/api/`, or `src/state/` to function, modifies the shared runtime core to serve its own needs, or duplicates runtime logic in a second language rather than sharing it through the trait interface.

## Normative Config and Interface Changes

### Environment Variables

The normative runtime configuration surface is:

| Variable | Purpose |
| :--- | :--- |
| `VEX_MODEL_URL` | Base URL of the model endpoint |
| `VEX_MODEL_NAME` | Model identifier string |
| `VEX_MODEL_TOKEN` | Optional token or key for authenticated endpoints |
| `VEX_MODEL_BACKEND` | Backend kind: `local-runtime` or `api-server` |
| `VEX_MODEL_PROTOCOL` | Wire protocol: `messages-v1` or `chat-compat` |
| `VEX_TOOL_CALL_MODE` | Tool invocation mode: `structured` or `tagged-fallback` |
| `VEX_STATE_DIR` | Durable task-state directory (default: `.vex/state`) |
| `VEX_POLICY_FILE` | Capability-policy file (default: `.vex/policy.toml`) |
| `VEX_WORKDIR` | Working directory for command execution |
| `VEX_MAX_TOKENS` | Maximum tokens per model request |
| `VEX_MAX_HISTORY_LINES` | Maximum rolling history retained |

The runtime architecture must not retain provider-branded variable names,
branded endpoint defaults, or vendor-specific model-name prefix validation.
The legacy provider-specific version field is removed entirely. Version-header
negotiation moves behind protocol-internal request building in each
`ModelProtocol` variant.

### Normative Enums

```rust
enum ModelBackendKind {
    LocalRuntime,
    ApiServer,
}

enum ModelProtocol {
    MessagesV1,
    ChatCompat,
}

enum ToolCallMode {
    Structured,
    TaggedFallback,
}

enum Capability {
    ReadFile,
    WriteFile,
    ApplyPatch,
    RunCommand,
    Network,
    Browser,
}

enum ApprovalScope {
    Once,
    Task,
    Session,
}

enum TaskStatus {
    Ready,
    Running,
    AwaitingApproval,
    Cancelling,
    Completed,
    Failed,
}
```

### Required Interfaces

```rust
pub trait ModelBackend: Send + Sync {
    fn backend_kind(&self) -> ModelBackendKind;
    fn protocol(&self) -> ModelProtocol;
    fn supports_structured_tools(&self) -> bool;
    fn is_local(&self) -> bool;
    async fn create_stream(&self, messages: &[ApiMessage]) -> Result<ByteStream>;
}

pub trait CommandRunner: Send + Sync {
    async fn run_one_shot(&self, req: CommandRequest) -> Result<CommandResult>;
    async fn run_streaming(
        &self,
        req: CommandRequest,
        tx: tokio::sync::mpsc::Sender<OutputChunk>,
    ) -> Result<CommandHandle>;
    async fn cancel(&self, handle: CommandHandle) -> Result<()>;
    async fn attach_pty(&self, req: CommandRequest) -> Result<PtySession>;
}

pub trait ApprovalPolicy {
    fn evaluate(&self, capability: Capability) -> PolicyAction;
    fn load_from_file(path: &std::path::Path) -> Result<Self>
    where
        Self: Sized;
}

pub struct ApprovalRequest {
    pub capability: Capability,
    pub scope: ApprovalScope,
    pub description: String,
    pub preview: Preview,
}

#[derive(Serialize, Deserialize)]
pub struct TaskState {
    pub id: TaskId,
    pub status: TaskStatus,
    pub active_grants: std::collections::HashMap<Capability, ApprovalScope>,
    pub changed_files: Vec<std::path::PathBuf>,
    pub command_history: Vec<CommandEvidence>,
    pub conversation_snapshot: ConversationCheckpoint,
    pub interrupted_sessions: Vec<InterruptedCommand>,
}
```

`CommandRunner` is greenfield work in this roadmap. No equivalent general
command runtime exists in the current codebase.

## Execution and Approval Model

The approval model is capability-based, not tool-name-based.

Default policy posture: repo-confined read, search, and list operations are
allowed by default; mutating edits and command execution are approval-gated by
default.

Approval scopes are `Once`, `Task`, and `Session`. Command execution must
support one-shot execution, streaming stdout and stderr, cancellation, and PTY
support when required by an interactive command.

Command and tool results must be surfaced as structured task evidence in the UI.
They are not permitted to exist only as model-generated summaries.

`ApprovalPolicy` is separate from `RuntimeCorePolicy`. `RuntimeCorePolicy`
continues to govern prompt-shaping and evidence-shaping concerns; it does not
become the approval mechanism.

A representative policy file shape is:

```toml
# .vex/policy.toml
# Values: "allow" | "deny" | "once" | "task" | "session"

[capabilities]
ReadFile   = "allow"
WriteFile  = "once"
ApplyPatch = "once"
RunCommand = "task"
Network    = "deny"
Browser    = "deny"
```

## Task State and Resume Model

Task state is durable on disk. The first milestone supports exactly one active
interactive task at a time.

Resume is explicit, not automatic. On restart, interrupted commands are marked
as `interrupted` rather than silently restored. Changed files are tracked as
task evidence throughout task execution. Task-scoped approvals survive resume.

State persistence must use atomic write semantics: serialize to a temporary
path, flush and close, then rename into the final state file path.

## TUI Direction

Ratatui remains the UI foundation. The interface shifts from chat-history-first
to task-execution-first, with the following persistent regions: header/status,
activity/audit trail, output pane for command output and diffs, and input pane.
Changed files must remain visible during active task execution. No editor-first
or full-screen editor workflow is introduced by this ADR.

This phase is sequenced after ADR-018 and depends on ADR-018's managed TUI and
streaming infrastructure.

A representative layout is:

```
+-------------------------------------+
| Header: task-id | status | backend  |
+-------------------------------------+
| Activity / Audit Trail              |
|  [ok] ReadFile: README.md           |
|  [?]  ApplyPatch: src/main.rs       |
|  [->] RunCommand: cargo test        |
+-------------------------------------+
| Output Pane                         |
| $ cargo test                        |
| running 12 tests...                 |
+-------------------------------------+
| Input: [prompt]  [y/n/s]            |
+-------------------------------------+
```

## Migration Plan

### Phase 1 — Neutralize config and model abstractions

**Objective:** remove legacy provider-branded variables, branded defaults, and
vendor-specific validation rules.

**Implementation direction:** replace the legacy config surface with the
normative `VEX_*` variables, remove branded endpoint defaults, remove
vendor-specific model-name validation, and rewrite docs, UI, and config examples
to neutral naming. Remove the legacy provider-specific version field and move
version-header negotiation behind protocol-internal request building rather than
user-facing configuration.

**Completion condition:** no provider-branded config names, branded defaults, or
vendor-specific model validation rules remain in tracked runtime configuration,
docs, or validation logic.

### Phase 2 — Add first-class command execution

**Objective:** introduce `CommandRunner` as a built-in capability.

**Implementation direction:** add a new general command-execution subsystem for
one-shot, streaming, cancellable, and PTY-capable execution. This is greenfield
work and is not a refactor of an existing shell subsystem.

**Completion condition:** commands can run, stream output, be cancelled, and
report structured evidence into the task model.

### Phase 3 — Make edits diff-native

**Objective:** require approval-gated diff preview for existing-file mutations.

**Implementation direction:** replace the direct, ungated existing-file mutation
flow in `src/tools/operator.rs` with previewable patch generation and explicit
approval before apply.

**Completion condition:** no existing file is silently rewritten; patch review is
required before mutation.

### Phase 4 — Introduce capability-based approval policy

**Objective:** enforce approvals through `ApprovalPolicy`.

**Implementation direction:** add capability-based policy evaluation and
structured approval requests, while keeping `ApprovalPolicy` separate from
`RuntimeCorePolicy`.

**Completion condition:** mutating capabilities and command execution are
enforced by the approval system, independent of prompt-shaping policy.

### Phase 5 — Add durable task state and resume

**Objective:** persist task progress and support explicit resume.

**Implementation direction:** serialize `TaskState` to disk across meaningful
state transitions and restore resumable tasks on operator request.

**Completion condition:** interrupted tasks can be resumed with preserved
evidence, changed-file tracking, and task-scoped approvals.

### Phase 6 — Rework the TUI around task execution

**Objective:** move the terminal UI from conversation-first to task-first.

**Implementation direction:** build the persistent task-execution layout on top
of ADR-018's managed TUI and streaming primitives.

**Completion condition:** the TUI exposes status, activity, output, approvals,
and changed files in the task-oriented layout.

### Phase 7 — Improve repo-navigation tooling

**Objective:** provide enough repo inspection capability for autonomous context
gathering.

**Implementation direction:** strengthen search, discovery, and read-oriented
tooling while keeping those operations repo-confined and approval-light.

**Completion condition:** the agent can gather repository context without
requiring operator intervention for routine inspection.

### Phase 8 — Defer browser automation to a later optional phase

**Objective:** preserve future extension space without widening first-milestone
scope.

**Implementation direction:** reserve `Capability::Browser` in the type system
and policy model, but do not ship browser automation in the first milestone.

**Completion condition:** browser capability exists only as reserved future
surface, not as shipped first-milestone behavior.

## Validation and Acceptance

### Acceptance Scenarios

1. Inspect a repository, patch code, run checks, and summarize the result using
   a local free/open model runtime.
2. Repeat the same workflow against a self-hosted neutral-compatible model
   server.
3. Interrupt a task, restart the application, and explicitly resume safely.
4. Review and approve a patch before apply.
5. Allow read-only repository inspection without repeated prompts while still
   prompting for mutating actions.
6. Confirm that docs, UI text, and config examples use only neutral free/open
   wording.

### Required Tests

Tests must cover: config parsing and validation, command execution and
cancellation, patch generation and application, approval policy enforcement,
task persistence and explicit resume, and TUI rendering of approvals, task
state, output, and diff previews.

Representative test intent:

```rust
#[test]
fn config_accepts_neutral_model_names() {
    std::env::set_var("VEX_MODEL_NAME", "local-model");
    std::env::set_var("VEX_MODEL_URL", "http://localhost:8080/v1");
    std::env::set_var("VEX_MODEL_BACKEND", "local-runtime");
    let cfg = Config::load().unwrap();
    assert!(cfg.validate().is_ok());
}

#[tokio::test]
async fn command_runner_streams_and_cancels() {
    let runner = DefaultCommandRunner::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let handle = runner
        .run_streaming(CommandRequest::new("sleep", &["10"]), tx)
        .await
        .unwrap();
    assert!(rx.recv().await.is_some());
    runner.cancel(handle).await.unwrap();
}

#[test]
fn approval_policy_is_capability_scoped() {
    let policy =
        FileApprovalPolicy::load_from_file(std::path::Path::new(".vex/policy.toml")).unwrap();
    assert!(matches!(policy.evaluate(Capability::ApplyPatch), PolicyAction::Prompt(_)));
    assert!(matches!(policy.evaluate(Capability::RunCommand), PolicyAction::Prompt(_)));
    assert!(matches!(policy.evaluate(Capability::ReadFile), PolicyAction::Allow));
}
```

## Consequences

### Benefits

- moves `vexcoder` toward a real coding-agent loop
- removes provider coupling from the public architecture
- improves auditability: every tool invocation, approval, and mutation is
  recorded as structured evidence
- improves recoverability: tasks survive process interruption and can be
  resumed explicitly
- keeps deployment self-hostable and free/open

### Tradeoffs

- more runtime complexity: async command execution, PTY handling, and state
  serialization add surface area
- more UI state complexity: the TUI must manage task status, approval prompts,
  streaming output, and diff previews concurrently
- stronger integration-test burden
- breaking config and docs migration: the `VEX_*` rename is a hard break for
  any deployment using legacy provider-branded variables; a migration guide must
  accompany Phase 1

## Compliance Notes for Agents

- Do not reintroduce provider-branded config names or examples.
- Do not add paid-service assumptions.
- Do not bypass capability-based approval for mutating operations.
- Do not perform hidden file rewrites where a diff preview is required.
- Do not add browser automation to the first milestone.
- Do not introduce native application packaging or new runtime surface implementations in first-milestone work. Any future milestone that introduces these must do so via a dedicated ADR. Packaging layers must not contain agent logic. New `RuntimeMode` implementations must call into the shared runtime core unchanged — they must not modify `src/runtime/`, `src/api/`, or `src/state/` to serve surface-specific needs.
- Do not conflate `RuntimeCorePolicy` (prompt-shaping) with `ApprovalPolicy`
  (capability gating); they are separate concerns and both must be maintained.
