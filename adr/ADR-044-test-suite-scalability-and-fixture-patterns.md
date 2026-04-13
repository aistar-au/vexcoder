# ADR-044: Test Suite Scalability and Fixture Patterns

- **Status:** Proposed
- **Date:** 2026-04-07
- **Deciders:** Core maintainer
- **Depends on:** ADR-041, ADR-043
- **Supersedes:** None
- **Superseded by:** None

## Context

The test suite currently passes 1301 tests grouped under `src/app/tests/` with
a flat file layout that is showing structural pressure:

| File | Lines | Concern |
| :--- | ---: | :--- |
| `task_layout.rs` | 999 | Exceeds the ~300-line guideline; mixes timeline, layout, and transcript fixture scenarios |
| `slash_commands.rs` | 721 | Nearing the ~300-line guideline; relies on ad hoc `ENV_LOCK` guards scattered across tests |
| `input.rs` | 673 | Approaches the ~300-line guideline |
| `overlay.rs` | 447 | Exceeds the ~300-line guideline |
| `memory.rs` | 365 | Exceeds the ~300-line guideline |
| `transcript.rs` | 325 | Exceeds the ~300-line guideline |

The test infrastructure has four variant `setup_ctx*` helpers in `setup.rs`
(`setup_ctx`, `setup_ctx_with_updates`,
`setup_ctx_with_responses_and_updates`, and a session-note variant) that
duplicate `mpsc` channel allocation and `MockApiClient` construction. Each
caller assembles nearly identical boilerplate. Tests that need an isolated
environment variable guard use raw `ENV_LOCK.blocking_lock()` +
`std::env::remove_var` pairs scattered across all callers rather than a
single RAII type.

The suite also lacks a single top-level integration-test aggregator under
`tests/`, a shared `tests/common/test_support.rs` surface for reusable SSE
fixtures and builder helpers, and a standard async runtime declaration for
streaming-heavy tests. The current branch carries roughly 50-70 focused tests
across `src/app/tests/`, `src/state/conversation/tests/`, and `tests/`; the
series target is to grow past 100 focused tests without reintroducing fixture
duplication or oversized generic test buckets.

As the `TaskDocumentCondenser` expands its event coverage (ADR-045), the
number of scenarios exercised in each of these files will grow. Without a
binding rule, individual test files will exceed 1000 lines and the duplicated
setup boilerplate will compound across
every new fixture added in PRs 4–7.

This ADR records the structural conventions that should govern the test suite
as it scales through the document-cutover series.

## Decision

### Rule 1 — File size ceiling

No test file under `src/app/tests/` or `tests/` may exceed approximately 300
lines. When a file approaches that ceiling, the author must split it by
functional boundary (e.g. by command group, by rendering concern, or by turn
lifecycle phase) before adding further tests. Splits must use the existing
`mod.rs` + named submodule pattern already established by
`tests/model_turn/` and `tests/session/`.

### Rule 2 — Shared aggregator and support module

The repository must grow a single integration-test aggregator at `tests/all.rs`
and a shared support surface under `tests/common/test_support.rs`.

The aggregator keeps integration-test entry points discoverable and stable:

- `mod suite;`
- `use vexcoder as _;`

The shared support surface owns reusable SSE fixtures, tagged tool-call
round-trip helpers, and any future builder types needed by renderer and
runtime tests. New helper modules must use descriptive domain names such as
`tool_rendering.rs`, `session_runtime.rs`, or `transcript_projection.rs`.
Generic buckets such as `misc.rs`, `helpers2.rs`, or `more_tests.rs` are not
permitted.

### Rule 3 — Single builder for RuntimeContext fixtures

The four `setup_ctx*` functions in `setup.rs` must be consolidated into a
single `MockContextBuilder` type. The builder exposes:

- `MockContextBuilder::new()` — returns a builder with defaults (no
  canned responses, no update receiver)
- `MockContextBuilder::with_responses(Vec<Vec<String>>)` — injects canned
  API responses for model-stream tests
- `MockContextBuilder::with_workdir(PathBuf)` — injects an explicit working
  directory when the scenario depends on path-sensitive rendering or tool IO
- `MockContextBuilder::build() -> RuntimeContext` — finalizes and returns the
  context, dropping the update channel
- `MockContextBuilder::build_with_updates() -> (RuntimeContext,
  UnboundedReceiver<UiUpdate>)` — finalizes and returns both

No test may call `mpsc::unbounded_channel`, `ApiClient::new_mock`, or
`ConversationManager::new_mock` directly. All fixture construction goes
through `MockContextBuilder`.

This rule applies to new tests. The four existing helpers may remain as
thin wrappers over `MockContextBuilder` until `task_layout.rs` is split
(PR 5), at which point the helpers are removed.

### Rule 4 — RAII environment guard

Tests that mutate environment variables must use a `TempEnv` RAII guard
rather than raw `ENV_LOCK` + `set_var`/`remove_var` pairs. `TempEnv` must:

- Acquire `ENV_LOCK` on construction.
- Record the previous value of every variable it overwrites.
- Restore all previous values on `Drop`, even if the test panics.
- Keep the guard and scratch directory in one owned type so cleanup is not
  split across helper layers.

The preferred shape is:

```rust
pub struct TempEnv {
    _guard: Arc<Mutex<()>>,
    _dir: TempDir,
}
```

Existing tests that use raw `ENV_LOCK.blocking_lock()` pairs must be
migrated to `TempEnv` before the `slash_commands.rs` file is split
(PR 4 follow-up or PR 5, whichever splits that file first).

### Rule 5 — Parameterised scenarios via `#[test_case]`

When three or more tests share the same assertion structure and differ only
in input values or setup flags, they must be collapsed into a single
`#[test_case]` parameterised test. This applies immediately to new tests.
A retroactive cleanup of existing repeated scenarios is
deferred to the split phase for the relevant file.

### Rule 6 — Async test runtime declaration

All async tests must use the `#[tokio::test]` attribute. No test may use
`Runtime::new().unwrap().block_on(…)` inline. Streaming, condenser, search, and
renderer integration tests should default to:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
```

Simple async unit tests that do not depend on scheduler behavior may keep the
plain `#[tokio::test]` form. No other async runtime is permitted in the test
suite.

### Rule 7 — Projection-aware test assertions

Tests added after PR 3 must assert against `project_transcript_rows` output
rather than against internal buffer fields. Direct assertions on
`history_state.lines`, stream segment buffers, or raw `current_turn_*`
fields are prohibited for new tests; they indicate the assertion is testing
implementation state rather than observable document behaviour.

### Rule 8 — Coverage and naming hygiene

The test-series follow-up must add a coverage metric lane and a naming pass:

- Add a `tarpaulin` coverage command to the documented validation flow or a
  dedicated CI lane before PR 7 closes.
- Review test names so the scenario and expected outcome are explicit from the
  function name alone.
- Keep new production and test files near the ~300-line ceiling whenever
  practical; split by domain boundary before introducing catch-all names.

Any new dependency introduced to support these rules remains subject to the
repository MIT-or-Apache-2.0 license policy.

## Phased rollout

### Phase 1 — foundation (target: first follow-up after PR 351, about 8 hours)

- Add `tests/all.rs` as the integration-test aggregator.
- Add `tests/common/test_support.rs` and move shared SSE and tagged-tool
  helpers into it.
- Introduce `TempEnv` and migrate the current ad hoc environment-variable
  callers.

### Phase 2 — fixture modernization (about 9 hours)

- Replace the `setup_ctx*` variants with `MockContextBuilder`.
- Adopt `with_responses`, `with_workdir`, `build`, and `build_with_updates`
  on new tests first, then migrate the existing callers as files split.
- Standardize async tests on `#[tokio::test(flavor = "multi_thread",
  worker_threads = 2)]` where scheduler behavior matters.

### Phase 3 — scalability and reporting (about 10 hours)

- Collapse repeated scenarios into `#[test_case]` tables.
- Add `tarpaulin` coverage reporting.
- Review test-function and module names for explicit, domain-specific wording.
- Track progress toward 100+ focused tests and a test-to-source ratio in the
  0.15-0.2 range as the document-cutover series lands.

## Consequences

- Each PR in the document-cutover series (PR 4–7) splits at least one
  oversized test file as a prerequisite for landing new tests, preventing
  unbounded growth.
- The suite gains a predictable `tests/all.rs` entry point and a reusable
  `tests/common/test_support.rs` layer instead of repeating SSE and context
  setup code across modules.
- `MockContextBuilder` reduces per-test boilerplate by roughly 5–8 lines and
  eliminates the risk of diverging mock setups.
- `TempEnv` removes the class of test-isolation bugs where `remove_var` is
  skipped on a panic or early-return path.
- `#[test_case]` parameterisation makes coverage gaps and missing edge cases
  visible at a glance rather than requiring a reader to compare near-identical
  test bodies.
- Explicit module and test names reduce the chance that future PRs re-create
  broad files such as `task_layout.rs` under new generic names.
- These rules apply specifically to `src/app/tests/` and `tests/`; upstream
  unit tests in individual `src/` modules are governed by the existing
  module-size ceiling and seam-coverage requirement from the architecture
  gate rather than by these fixture conventions.

## Validation targets

Files that must be split as part of the PR 4–7 series:
- `src/app/tests/task_layout.rs` (999 lines — split in PR 5 or earlier)
- `src/app/tests/slash_commands.rs` (721 lines — split when ENV_LOCK
  migration completes)
- `src/app/tests/input.rs` (673 lines — split at PR 4 if tests are added)
- `src/app/tests/overlay.rs` (447 lines — split when approval tests expand)
- `src/app/tests/memory.rs` (365 lines — split when session-note tests expand)
- `src/app/tests/transcript.rs` (325 lines — split when projection tests expand)
