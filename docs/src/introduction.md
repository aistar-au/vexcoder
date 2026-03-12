# VexCoder

<div class="vex-hero">
<p><strong>VexCoder</strong> is a terminal-first coding assistant. It streams responses from a
language model, executes shell tools on your behalf, and renders everything in an
interactive terminal UI built with <a href="https://github.com/ratatui-org/ratatui">Ratatui</a>.</p>
</div>

## Get started — build from source

The fastest path to a running session. Requires the Rust stable toolchain
([rustup](https://rustup.rs)) and a running inference endpoint.

```bash
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
cargo build --release
VEX_MODEL_URL=http://localhost:8000/v1/messages ./target/release/vex
```

Or install directly into your Cargo bin path:

```bash
cargo install --path .
VEX_MODEL_URL=http://localhost:8000/v1/messages vex
```

For full platform instructions see the **[Installation guide](installation/index.md)**.

---

## Key properties

<div class="vex-features">
<div class="vex-feature">
<h3>Deterministic dispatch</h3>
<p>Every tool call follows a single, explicit approval path. No hidden routing
alternatives. One path from user input to tool execution and back.</p>
</div>
<div class="vex-feature">
<h3>Dual-protocol support</h3>
<p>Native <code>messages-v1</code> and OpenAI-compatible <code>chat-compat</code>. Protocol is
inferred from the endpoint URL — the same binary works against local inference
servers and hosted remote APIs.</p>
</div>
<div class="vex-feature">
<h3>Headless and TUI modes</h3>
<p>The same core runtime powers both an interactive Ratatui terminal session and
a headless mode for scripting and CI pipelines.</p>
</div>
<div class="vex-feature">
<h3>No cloud, no telemetry</h3>
<p>VexCoder is a local binary that connects to whichever inference endpoint you
configure. No account, no tracking, no network requirement beyond your model
endpoint.</p>
</div>
</div>

---

## Documentation sections

| Section | What you will find |
|:---|:---|
| [Quick Start](quick-start.md) | From zero to a running session |
| [Installation](installation/index.md) | macOS, Linux, Windows — build from source and pre-built binaries |
| [Configuration](configuration.md) | All environment variables and TOML config keys |
| [TUI Commands](commands.md) | Every `/command` you can type in a session |
| [Architecture Overview](architecture.md) | Runtime structure and dispatch path |
| [Migration Guide](migration.md) | Upgrading from pre-ADR-022 deployments |
| [ADRs](adr/index.md) | Architecture Decision Records |

---

## Source layout

```
src/
  bin/vex.rs          entry point
  app.rs              command and mode surface
  runtime/            orchestration and policy wiring
  state/              conversation and task-state persistence
  tools/              tool execution and workspace confinement
  api/                HTTP client, streaming parser, protocol detection
  ui/                 Ratatui render loop and layout
```

Architecture decisions are recorded incrementally in [`docs/adr/`](adr/index.md).

---

<div class="vex-sponsor">
<p>
<strong>Sponsor VexCoder</strong><br>
SegWit &nbsp;<code>bc1qrv27qmjvleyrllr3ed7pxstxgvrjesxxj0dzwa</code><br>
Ethereum &nbsp;<code>0xe5D746f089D155f0E1C6dD6C663E3F5D853BAe6a</code>
</p>
</div>
