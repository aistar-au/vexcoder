# ADR-002: Lexical Path Normalization

**Status:** Accepted  

## Decision

- All file paths stored and compared in lexically normalized form (`std::path::PathBuf::canonicalize` or `Path::components`).
- No runtime resolves symlinks speculatively; normalize at the tool boundary only.

## References

- Rust std [`std::path`](https://doc.rust-lang.org/std/path/)
