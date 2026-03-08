# Quick Start

This page gets you from zero to a running session in the fewest steps. For full platform-specific prerequisites and build options, see the [Installation](installation/index.md) guide.

## Prerequisites

- Rust stable toolchain (1.75 or later). Install via [rustup](https://rustup.rs).
- A running inference endpoint that accepts either the `messages-v1` or `chat-compat` (OpenAI-compatible) API.

## Run against a local endpoint

```bash
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
VEX_MODEL_URL=http://localhost:8000/v1/messages cargo run
```

VexCoder infers the protocol from the URL. An endpoint path containing `/v1/messages` uses `messages-v1`. An endpoint path containing `/chat/completions` or ending in `/v1` uses `chat-compat`.

## Run against a remote endpoint

```bash
VEX_MODEL_URL=https://your-inference-server/v1/messages \
VEX_MODEL_TOKEN=your-token \
VEX_MODEL_NAME=your-model-name \
cargo run
```

## Verify the build gate passes

Before running in a development context, confirm the full gate is green:

```bash
make gate-fast
```

This runs `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, and `cargo test --all-targets`. A green gate is the baseline for any code contribution.

## Next steps

- [Configuration reference](configuration.md) — all environment variables
- [TUI Commands](commands.md) — what you can type inside the session
- [Installation](installation/index.md) — platform-specific release builds
