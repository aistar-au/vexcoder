# Build From Source

VexCoder is a local coding assistant CLI you build and run from a normal Rust
checkout. Start here if you only need a working binary; architecture notes and
ADR history are later in the book.

Prerequisite: a stable Rust toolchain with `cargo` available on your `PATH`.

## Fast path

```bash
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
cargo build --release
./target/release/vex init
./target/release/vex
```

The built binary is at `target/release/vex`.

If your endpoint requires authentication, export a token before launching:

```bash
export VEX_MODEL_TOKEN="your-token"
```

## Next

- [Quick Start](quick-start.md) for endpoint configuration and first-run examples
- [Configuration](configuration.md) for the full `.vex/config.toml` surface
- [CLI and TUI Commands](commands.md) for interactive and batch usage
- [Architecture Overview](architecture.md) when you need the code-layout and boundary view
