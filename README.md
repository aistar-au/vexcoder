# VexCoder

Interactive coding assistant CLI, implemented in Rust.

## Documentation

Full documentation is in [`docs/src/`](docs/src/SUMMARY.md). To read it locally:

```bash
cargo install mdbook
mdbook serve docs
```

- [Introduction](docs/src/introduction.md)
- [Architecture Overview](docs/src/architecture.md)
- [Quick Start](docs/src/quick-start.md)
- [Configuration](docs/src/configuration.md)
- [CLI and TUI Commands](docs/src/commands.md)
- [Dependency Upgrades](docs/src/dependency-upgrades.md)

Architecture records are stored under [`adr/`](adr/ADR-README.md). They are kept in
the repository for design history, but are not part of the published user
guide. The current runtime, application, transport, and renderer dependency
layout is summarized in [`docs/src/architecture.md`](docs/src/architecture.md),
with the ADR set under [`adr/`](adr/ADR-README.md) carrying the detailed design
history. Direct crate version requirements are centralized in the root
[`Cargo.toml`](Cargo.toml) `workspace.dependencies` table, and the maintainer
workflow for stale-version audits, security checks, and manifest upgrades is
documented in [`docs/src/dependency-upgrades.md`](docs/src/dependency-upgrades.md).
Use `make deps-deny`, `make deps-audit`, `make deps-plan`, and `make deps-upgrade`
(backed by `cargo-deny`, `cargo-outdated`, and `cargo-upgrade`) for all
dependency work. `make bump` changes the version.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md).

---

Sponsor: SegWit `bc1qrv27qmjvleyrllr3ed7pxstxgvrjesxxj0dzwa` · Eth `0xe5D746f089D155f0E1C6dD6C663E3F5D853BAe6a`
