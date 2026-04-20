# VexCoder

Interactive coding assistant CLI, implemented in Rust.

## Build from source

Requires a stable Rust toolchain with `cargo` on your `PATH`.

```bash
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
cargo build --release
./target/release/vex init
./target/release/vex
```

The built binary is at `target/release/vex`.

Set your model endpoint in `.vex/config.toml`, and export
`VEX_MODEL_TOKEN` only when the endpoint requires one.

## Documentation

Full documentation is in [`docs/src/`](docs/src/SUMMARY.md). To read it locally:

```bash
cargo install mdbook
mdbook serve docs
```

Start with:

- [Build From Source](docs/src/introduction.md)
- [macOS](docs/src/macos.md)
- [Linux](docs/src/linux.md)
- [Windows PowerShell](docs/src/windows.md)

Architecture records are stored under [`adr/`](adr/ADR-README.md). They carry
design history and follow-up amendments separately from the build-first user
guide.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md).

---

Sponsor: SegWit `bc1qrv27qmjvleyrllr3ed7pxstxgvrjesxxj0dzwa` · Eth `0xe5D746f089D155f0E1C6dD6C663E3F5D853BAe6a`
