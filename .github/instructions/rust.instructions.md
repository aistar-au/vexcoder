---
applyTo: "**/*.rs,**/Cargo.toml,**/Cargo.lock"
---

## Rust-specific guidance

### Code style

- Follow idiomatic Rust patterns already established in the repository.
- Prefer small, composable functions over large control-heavy blocks.
- Avoid unnecessary cloning, allocation, and ownership churn.
- Prefer borrowing when it keeps code clear and correct.
- Use explicit types where they improve readability at module boundaries.
- Keep public API changes minimal and document them in the pull request.

### Error handling

- Keep error messages actionable and specific. Include relevant context such as
  file paths, field names, or expected values.
- In library or reusable code, prefer returning errors over panicking.
- Propagate errors with `?` and context (via `anyhow::Context`, `map_err`, or
  equivalent) rather than discarding them.
- Reserve `unwrap` and `expect` for cases where the invariant is proven by
  construction and documented in a comment.

### Safety and correctness

- Avoid `unsafe` unless the task explicitly requires it. When `unsafe` is
  necessary, document the safety invariant in a `// SAFETY:` comment.
- Validate all indices, lengths, and type conversions at trust boundaries.
- Prefer `TryFrom`/`TryInto` over `as` casts for numeric conversions that may
  lose precision or sign.

### Testing

- When changing behavior, add or update tests near the affected code.
- Prefer deterministic tests. Avoid timing-dependent assertions without
  explicit tolerance or retry logic.
- Keep test helpers minimal and co-located with the tests that use them.

### Dependencies

- Prefer existing workspace dependencies over adding new crates.
- Keep direct dependency version requirements centralized in the root
  `[workspace.dependencies]` table and inherit them with `workspace = true`
  where possible. This is the single source of truth for every version
  requirement; future bumps touch one line, not N crate manifests.
- When a dependency bump needs Rust source edits, consult
  `[workspace.metadata.upgrade-seams]` and `[workspace.metadata.upgrade-notes]`
  in the root `Cargo.toml` first. Those tables are the reviewed map of which
  files are intended to absorb API churn.
- When adding a dependency, justify it in the pull request and prefer crates
  with minimal transitive dependency trees. Run `make deps-deny` after adding
  to confirm the new crate's license is on the allow-list in `deny.toml`.
- Follow the workspace's existing version convention (semver ranges such as
  `"1"` or `"0.8"`). Rely on `Cargo.lock` and CI to detect breakage.
- Use `cargo upgrade` to change manifest requirements and `cargo update` to
  refresh `Cargo.lock`; they solve different parts of an upgrade and must not
  be confused. `cargo update` alone never changes `Cargo.toml`.
- Run `make deps-deny` (cargo-deny: RustSec advisories, license compliance,
  wildcard bans) before and after any upgrade batch. See `deny.toml` for the
  full configuration and `docs/src/dependency-upgrades.md` for workflow detail.
- When an API seam needs source changes for a version bump, confine them to the
  designated seam files documented in `docs/src/dependency-upgrades.md` rather
  than scattering workarounds through unrelated modules.
- Do not add facade layers for `serde` derives or Tokio attribute macros just
  to hide them. Those are compile-time annotations; keep them direct unless a
  specific maintenance problem justifies a different pattern.

### Dependency upgrade workflow (summary)

```bash
make deps-deny                             # security + license gate
make deps-audit                            # find stale direct deps
make deps-plan ARGS='-p <crate>@<version>' # dry-run manifest edit
make deps-upgrade ARGS='-p <crate>@<version>' # apply + cargo update + check
make deps-deny                             # re-run gate after upgrade
make gate                                  # full CI gate before pushing
```

Full details: `docs/src/dependency-upgrades.md`

### Review checklist

Before submitting Rust changes, verify:

- [ ] Correctness — logic matches intent for all reachable paths
- [ ] State transitions — ownership moves and mutability are intentional
- [ ] Error propagation — errors surface with enough context to diagnose
- [ ] Lifetimes and ownership — no unnecessary cloning or leaking
- [ ] Test coverage — changed behavior has at least one covering test
- [ ] Formatting and lint cleanliness — `cargo fmt` and `cargo clippy` pass
- [ ] Dependency changes — new crates justified, `make deps-deny` passes
