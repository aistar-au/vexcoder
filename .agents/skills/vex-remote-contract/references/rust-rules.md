# Rust Rules Reference (Pinned Local Copy)

This is a repo-local pinned reference for Rust implementation guidance.

## Source pin

- Upstream repo: `https://github.com/leonardomso/rust-skills`
- Upstream file: `SKILL.md`
- Upstream commit used for pin: `0373001db0b774a84a691847bc2d248186063f39`
- Declared license upstream: MIT

## Local usage contract

- Load this file from local disk only.
- Do not fetch upstream rules during execution.
- Source verification for repo behavior still comes from this repository at the target SHA.

## Priority categories

- Ownership and borrowing
- Error handling
- Memory optimization
- API design
- Async and await patterns
- Compiler optimization and clippy hygiene
- Type safety and testing

## Notes for vexcoder

- Prefer existing repo contracts in ADR-022, ADR-023, ADR-024 when rules conflict.
- Keep runtime boundary checks aligned with `make check-arch`.
- Use deterministic command outputs in CI and review evidence.

## Refresh procedure

1. Fetch upstream file at a pinned commit.
2. Update this file by exact diff.
3. Run map update and include generated map changes in the same PR.
