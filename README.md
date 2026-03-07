# VexCoder

Terminal-first coding assistant with streaming responses, tool execution, and ratatui UI.

## Installation

### From Source

macOS/Linux:

```bash
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
make gate-fast
cargo build --release
./target/release/vex
```

Windows PowerShell 7:

```powershell
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo build --release --bin vex
.\target\release\vex.exe
```

To package a Windows archive locally, install Visual Studio Build Tools with the C++ workload and run:

```powershell
.\scripts\release.ps1 -Version v0.1.0-alpha.1 -Target x86_64-pc-windows-msvc -RunGate
```

### From GitHub Releases

Download the archive for your platform from the GitHub Releases page, unpack it, and run `vex`.

macOS/Linux:

```bash
curl -L -o vex.tar.gz https://github.com/aistar-au/vexcoder/releases/download/v0.1.0-alpha.1/vex-0.1.0-alpha.1-x86_64-unknown-linux-musl.tar.gz
tar -xzf vex.tar.gz
./vex-0.1.0-alpha.1-x86_64-unknown-linux-musl/vex
```

Windows PowerShell 7:

```powershell
Invoke-WebRequest -Uri "https://github.com/aistar-au/vexcoder/releases/download/v0.1.0-alpha.1/vex-0.1.0-alpha.1-x86_64-pc-windows-msvc.zip" -OutFile vex.zip
Expand-Archive vex.zip -DestinationPath .
.\vex-0.1.0-alpha.1-x86_64-pc-windows-msvc\vex.exe
```

Windows alpha archives are unsigned today. SmartScreen will show an "Unknown Publisher" warning until Authenticode signing is added. SignPath.io is the planned first signing path for open-source release automation.

## Quick Start

```bash
cargo run
```

## Configuration

`vexcoder` is configured via environment variables. `VEX_MODEL_URL` is the only required variable.

| Variable | Required | Description |
|---|---|---|
| `VEX_MODEL_URL` | Yes | API endpoint URL |
| `VEX_MODEL_TOKEN` | Remote only | Bearer token for non-local endpoints |
| `VEX_MODEL_NAME` | No | Model identifier (default: `local/default`) |
| `VEX_MODEL_PROTOCOL` | No | `messages-v1` or `chat-compat` (inferred from URL if omitted) |
| `VEX_TOOL_CALL_MODE` | No | `structured` (remote default) or `tagged-fallback` (local default) |
| `VEX_MODEL_BACKEND` | No | `api-server` or `local-runtime` (inferred from URL if omitted) |
| `VEX_WORKDIR` | No | Working directory override (defaults to current directory) |

`VEX_MODEL_PROTOCOL` is inferred from the URL: endpoints containing `/chat/completions` or ending in `/v1` default to `chat-compat`; all others default to `messages-v1`.

Local endpoint example:

```bash
VEX_MODEL_URL=http://localhost:8000/v1/messages \
VEX_MODEL_NAME=local/default \
cargo run
```

Remote endpoint example:

```bash
VEX_MODEL_URL=https://your-inference-server/v1/messages \
VEX_MODEL_TOKEN=your-token \
VEX_MODEL_NAME=your-model-name \
cargo run
```

For operators migrating from a pre-ADR-022 deployment, see `docs/src/migration.md`.

## Built-in TUI Commands

- `/commands` or `/help`
- `/clear`
- `/history`
- `/repo`
- `/ps`
- `/quit`

## Documentation

This repository uses mdBook + GitHub Pages for documentation.

- Config: `docs/book.toml`
- Pages: `docs/src/`
- Build locally: `mdbook build docs`

ADR files are stored under `TASKS/`, not under `docs/`.

Source maps:

- App/raw links for the Rust application code: `CONTRIBUTING.md`
- Full repository raw URL map: `TASKS/completed/REPO-RAW-URL-MAP.md`
- Sponsor VexCoder: SegWit bc1qrv27qmjvleyrllr3ed7pxstxgvrjesxxj0dzwa, Eth 0xe5D746f089D155f0E1C6dD6C663E3F5D853BAe6a

