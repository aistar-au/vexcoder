# VexCoder

Interactive coding assistant CLI, implemented in Rust.

## Build from source

Requires Git, a stable Rust toolchain with `cargo` on your `PATH`, write access in the checkout so `vex init` can create `.vex/` and `AGENTS.md`, and a reachable model endpoint.

`vex` does not bundle a model runtime. For the fastest same-machine setup, point `.vex/config.toml` at a local server on `http://127.0.0.1:8080/v1`. Local and private-network endpoints can stay on plain HTTP and do not need `VEX_MODEL_TOKEN`; remote public endpoints must use `https://` and a token.

```bash
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
cargo build --release
./target/release/vex init
```

The built binary is at `target/release/vex`.

The OS-specific guides under `docs/src/` walk through the local-server config, token rules, `vex doctor`, and the first interactive launch in detail.

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
