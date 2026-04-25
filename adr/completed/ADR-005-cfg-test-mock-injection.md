# ADR-005: `cfg(test)` Mock Injection

**Status:** Accepted  

## Decision

- Test doubles injected via `#[cfg(test)]` module boundaries; no runtime feature flags.
- `src/test_support.rs` exposes `EnvLockGuard`, `EnvRestore`, and `ENV_LOCK` for env-mutation safety.
- Integration tests must not depend on `#[cfg(test)]`-gated library items; use typed guards from test entry files.

## References

- Rust Reference: [Conditional compilation](https://doc.rust-lang.org/reference/conditional-compilation.html)
