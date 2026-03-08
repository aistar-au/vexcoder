# Introduction

VexCoder is a terminal-first coding assistant. It streams responses from a language model, executes shell tools on your behalf, and renders everything in an interactive terminal UI built with Ratatui.

## Key properties

VexCoder is designed around three invariants.

**Deterministic dispatch.** Every tool call follows an explicit approval path. There are no hidden routing alternatives. The runtime enforces a single canonical path from user input to tool execution and back.

**Dual-protocol support.** VexCoder speaks both a native `messages-v1` protocol and an OpenAI-compatible `chat-compat` protocol. The protocol is inferred from the endpoint URL so the same binary works against local inference servers and hosted remote APIs without reconfiguration.

**Headless and TUI modes.** The same core runtime powers both an interactive Ratatui terminal session and a headless mode suitable for scripting and CI pipelines. The mode boundary is enforced at the runtime seam, not via conditional branches inside business logic.

## What VexCoder is not

VexCoder is not a cloud service. It is a local binary that connects to whichever inference endpoint you configure. There is no telemetry, no account, and no network requirement other than the connection to your model endpoint.

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

Architecture decisions are recorded incrementally in [`docs/adr/`](../adr/ADR-README.md). Start with the ADR README for a map of what each record covers.
