# Installation

VexCoder is distributed as a single statically-linked binary. You can build it from source or download a pre-built archive from GitHub Releases.

## Choose your path

- [Build from source on macOS](macos.md)
- [Build from source on Linux](linux.md)
- [Build from source on Windows](windows.md)

## Download a pre-built release

Pre-built archives are attached to each [GitHub Release](https://github.com/aistar-au/vexcoder/releases).

**macOS / Linux:**

```bash
curl -L -o vex.tar.gz \
  https://github.com/aistar-au/vexcoder/releases/download/v0.1.0-alpha.1/vex-0.1.0-alpha.1-x86_64-unknown-linux-musl.tar.gz
tar -xzf vex.tar.gz
./vex-0.1.0-alpha.1-x86_64-unknown-linux-musl/vex
```

**Windows PowerShell 7:**

```powershell
Invoke-WebRequest `
  -Uri "https://github.com/aistar-au/vexcoder/releases/download/v0.1.0-alpha.1/vex-0.1.0-alpha.1-x86_64-pc-windows-msvc.zip" `
  -OutFile vex.zip
Expand-Archive vex.zip -DestinationPath .
.\vex-0.1.0-alpha.1-x86_64-pc-windows-msvc\vex.exe
```

Windows archives are currently unsigned. SmartScreen will display an "Unknown Publisher" warning. Authenticode signing via SignPath.io is planned for a future release.

## Supported targets

| Target | Notes |
|---|---|
| `x86_64-unknown-linux-musl` | Fully static, runs on any glibc-free Linux |
| `x86_64-pc-windows-msvc` | Requires Visual C++ Redistributable |
| `x86_64-apple-darwin` | macOS 11+ |
| `aarch64-apple-darwin` | Apple Silicon, macOS 11+ |
