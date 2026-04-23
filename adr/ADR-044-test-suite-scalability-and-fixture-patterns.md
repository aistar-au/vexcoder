# ADR-044: Test Suite Scalability and Fixture Patterns

**Status:** Proposed  
**Chain:** ADR-041, ADR-043

## Context

Test files exceeded 300 lines with duplicated setup helpers, inconsistent async patterns, and missing parameterization, producing maintenance friction as coverage grew.

## Decision

- No test file exceeds ~300 lines; split by functional boundary before reaching the ceiling.
- Single integration-test aggregator at `tests/all.rs`; shared helpers at `tests/common/test_support.rs`.
- Consolidate `setup_ctx*` variants into a single `MockContextBuilder` type.
- RAII `TempEnv` guard for environment-variable mutations; replaces scattered `unsafe` env calls.
- Collapse repeated scenarios into `#[test_case]` parameterized tests via [`test-case`](https://docs.rs/test-case).
- All async tests use `#[tokio::test]`.
- New tests assert against `project_transcript_rows` output, not internal buffer fields.
- Add [`cargo-tarpaulin`](https://docs.rs/cargo-tarpaulin) coverage reporting before the series closes.

## References

- [`test-case`](https://docs.rs/test-case) — parameterized test macros
- [`tokio`](https://docs.rs/tokio) — `#[tokio::test]` async test runtime
- [`tempfile`](https://docs.rs/tempfile) — temporary file fixtures
