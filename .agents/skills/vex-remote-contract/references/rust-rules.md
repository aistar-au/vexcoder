# Rust Rules Reference (Pinned Local Copy)

This is a repo-local pinned reference for Rust implementation guidance.

## Local maintenance

- This file is maintained locally as part of the vexcoder agent skill set.
- Rules are updated by explicit dispatcher-authored patches only.
- Do not fetch or merge upstream content automatically.

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

1. Update this file by exact diff authored in a dispatcher-owned branch.
2. Run map update and include generated map changes in the same PR.
