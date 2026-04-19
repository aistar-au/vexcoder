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
- [Privacy](docs/src/privacy.md)
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
The same manifest also carries `workspace.metadata.upgrade-seams` and
`workspace.metadata.upgrade-notes`, which are the maintainer map for the small
set of Rust files that should absorb dependency API churn.
Use `make deps-deny`, `make deps-audit`, `make deps-plan`, and `make deps-upgrade`
(backed by `cargo-deny`, `cargo-outdated`, and `cargo-upgrade`) for all
dependency work. `make bump` changes the version.

## Standards posture

The transport surface aligns with RFC 9110, RFC 9111, RFC 9112, RFC 8259, the
WHATWG server-sent-events parsing rules, RFC 7519, and RFC 8446 for the parts
implemented in-tree. One deliberate exception remains: upstream streamed model
requests use `POST` with EventSource framing because the provider APIs require
request bodies. Raw JSON chunk streams without SSE framing are unsupported and
discouraged.

The current client-side and LocalApiServer privacy posture is documented in
[`docs/src/privacy.md`](docs/src/privacy.md), including local storage paths,
credential handling, telemetry boundaries, and the read-only `/v1/privacy`
metadata endpoint.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md).

---

Sponsor: SegWit `bc1qrv27qmjvleyrllr3ed7pxstxgvrjesxxj0dzwa` · Eth `0xe5D746f089D155f0E1C6dD6C663E3F5D853BAe6a`
