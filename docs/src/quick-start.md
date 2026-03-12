# Quick Start

Get from zero to a running session in the fewest steps. For full platform-specific
prerequisites and build options, see the [Installation](installation/index.md) guide.

## Prerequisites

- Rust stable toolchain (1.75 or later) — install via [rustup](https://rustup.rs).
- A running inference endpoint that accepts either the `messages-v1` or
  `chat-compat` (OpenAI-compatible) API.

## Build from source

```bash
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
cargo build --release
```

The binary lands at `target/release/vex`. Alternatively, install it into your
Cargo bin path:

```bash
cargo install --path .
```

## Run against a local endpoint

```bash
VEX_MODEL_URL=http://localhost:8000/v1/messages ./target/release/vex
```

VexCoder infers the protocol from the URL:

- A path containing `/v1/messages` → `messages-v1`.
- A path containing `/chat/completions` or ending in `/v1` → `chat-compat` (OpenAI-compatible).

## Run against a remote endpoint

```bash
VEX_MODEL_URL=https://your-inference-server/v1/messages \
VEX_MODEL_TOKEN=your-token \
VEX_MODEL_NAME=your-model-name \
./target/release/vex
```

## Verify the build gate

Before running in a development context, confirm the full gate is green:

```bash
make gate-fast
```

This runs `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, and
`cargo test --all-targets`. A green gate is the baseline for any code contribution.

## Next steps

- [Configuration reference](configuration.md) — all environment variables and config keys
- [TUI Commands](commands.md) — everything you can type inside a session
- [Installation](installation/index.md) — full platform-specific build instructions
