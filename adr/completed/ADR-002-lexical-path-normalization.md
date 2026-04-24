# ADR-002: Lexical Path Normalization

**Status:** Accepted  

## Decision

- Tool paths are lexically normalized with `Path::components()` before workspace checks or policy evaluation.
- `fs::canonicalize` is reserved for the working directory and the nearest existing ancestor during symlink-escape checks.
- No runtime path resolution performs speculative filesystem traversal outside those guard checks.

## References

- Rust std [`std::path`](https://doc.rust-lang.org/std/path/)
- Rust std [`std::fs::canonicalize`](https://doc.rust-lang.org/std/fs/fn.canonicalize.html)
