# VexCoder

Terminal-first coding assistant — Rust · Ratatui · streaming tool execution.

## Build from source

```bash
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
cargo build --release
VEX_MODEL_URL=http://localhost:8000/v1/messages ./target/release/vex
```

Or install directly:

```bash
cargo install --path .
VEX_MODEL_URL=http://localhost:8000/v1/messages vex
```

Requires the Rust stable toolchain (≥ 1.75) via [rustup](https://rustup.rs) and a running inference endpoint.

## Documentation

Full documentation — including platform-specific installation, configuration reference, and TUI commands — is published on **[GitHub Pages](https://aistar-au.github.io/vexcoder/)** and lives in [`docs/src/`](docs/src/SUMMARY.md).

To read locally:

```bash
mdbook serve docs
```

Sections:

- [Introduction](docs/src/introduction.md)
- [Quick Start](docs/src/quick-start.md)
- [Installation — macOS, Linux, Windows](docs/src/installation/index.md)
- [Configuration](docs/src/configuration.md)
- [TUI Commands](docs/src/commands.md)
- [Architecture Overview](docs/src/architecture.md)
- [Architecture Decision Records](docs/adr/ADR-README.md)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md).

---

Sponsor: SegWit `bc1qrv27qmjvleyrllr3ed7pxstxgvrjesxxj0dzwa` · Eth `0xe5D746f089D155f0E1C6dD6C663E3F5D853BAe6a`
