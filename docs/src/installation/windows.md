# Build from Source — Windows

Tested on Windows 10 21H2 and Windows 11, using PowerShell 7 and the MSVC toolchain. WSL 2 is a viable alternative and follows the [Linux guide](linux.md) exactly.

## Prerequisites overview

- PowerShell 7 (not Windows PowerShell 5.1)
- Visual Studio Build Tools with the Desktop development with C++ workload
- Rust stable toolchain via rustup
- taplo (TOML validator)

## Step 1 — Install PowerShell 7

PowerShell 7 is required. The release scripts and some Makefile paths use syntax not available in Windows PowerShell 5.1.

Install via winget:

```powershell
winget install --id Microsoft.PowerShell --source winget
```

Or download the MSI installer from the [PowerShell GitHub releases page](https://github.com/PowerShell/PowerShell/releases).

Open a new PowerShell 7 window (`pwsh.exe`) for all remaining steps.

Verify:

```powershell
$PSVersionTable.PSVersion
# Major should be 7 or later
```

## Step 2 — Install Visual Studio Build Tools

The MSVC linker and Windows SDK headers are required. The free Build Tools installer provides everything needed without a full Visual Studio installation.

Download the Build Tools installer from the [Visual Studio downloads page](https://visualstudio.microsoft.com/downloads/) (scroll to "Tools for Visual Studio", then "Build Tools for Visual Studio").

Run the installer and select the **Desktop development with C++** workload. The required components are:

- MSVC v143 (or later) build tools
- Windows 11 (or 10) SDK
- C++ CMake tools (optional but recommended)

The installation requires approximately 6 GB of disk space.

Verify from a new PowerShell 7 window:

```powershell
cl.exe /?
# should print Microsoft C/C++ compiler version info
# if not found, ensure "Developer PowerShell" or add MSVC bin to PATH
```

Alternatively, use the "Developer PowerShell for VS" shortcut which sets the MSVC paths automatically.

## Step 3 — Install Rust via rustup

Download and run the rustup installer:

```powershell
Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile rustup-init.exe
.\rustup-init.exe
```

Select option 1 (default installation). The default Windows toolchain is `stable-x86_64-pc-windows-msvc`. Close and reopen PowerShell 7 after installation.

Verify:

```powershell
rustup show
cargo --version
```

Ensure the Cargo bin directory is on your path. The installer adds it to the user `PATH` automatically; if not:

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
```

Add this line to your PowerShell profile to persist it:

```powershell
Add-Content -Path $PROFILE -Value '$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"'
```

## Step 4 — Install taplo

```powershell
cargo install taplo-cli --locked
taplo --version
```

## Step 5 — Clone and build

```powershell
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
```

Build the release binary:

```powershell
cargo build --release --bin vex
```

The binary is at `target\release\vex.exe`.

The `make gate-fast` target requires GNU `make`. If you have it installed (via Git for Windows or Scoop), run it to confirm the gate is green:

```powershell
make gate-fast
```

Otherwise run the individual commands directly:

```powershell
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo test --all-targets
```

## Step 6 — Run

```powershell
$env:VEX_MODEL_URL = "http://localhost:8000/v1/messages"
.\target\release\vex.exe
```

## Step 7 — (Optional) Build a release archive

To produce the same `.zip` archive used by GitHub Releases, install the Visual Studio Build Tools C++ workload (Step 2) and run:

```powershell
.\scripts\release.ps1 -Version v0.1.0-alpha.1 -Target x86_64-pc-windows-msvc -RunGate
```

The archive is written to `dist\`.

Note: release archives are currently unsigned. SmartScreen will display an "Unknown Publisher" warning when you try to run the extracted binary. This is expected until Authenticode signing is added. You can bypass it by right-clicking the binary, selecting Properties, and checking Unblock.

## Troubleshooting

**`error: linker 'link.exe' not found`** — The MSVC Build Tools are not installed or not on PATH. Install the Desktop development with C++ workload (Step 2) and open a new Developer PowerShell or add the MSVC bin path to your environment.

**`cargo fmt --check` fails** — Run `cargo fmt` and re-run the check. The formatter output is canonical; manual line-width adjustments will be overwritten.

**`rustup-init.exe` requests admin elevation** — This is normal for the first-time install. The toolchain itself is installed per-user and does not require admin after the initial setup.
