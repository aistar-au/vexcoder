# Build environment

This guide documents the local build environment used to validate `vexcoder` from source and to maintain the mdBook documentation site.

## What this guide covers

- Windows 11 with PowerShell 7 and a Windows-native repo checkout
- Linux source builds
- macOS source builds
- Optional docs tooling such as GitHub CLI and mdBook
- Local preview and automatic GitHub Pages publication from `main`

## Shared expectations

Use a normal Git checkout of the repository and keep the Rust toolchain on the machine that will run the build.

Common repo checks:

```sh
git status
git branch --show-current
git remote -v
```

For docs work, the book lives in `docs/` and builds with:

```sh
mdbook build docs
```

## Windows 11 and PowerShell 7

This is the exact Windows-native path that was debugged in this session.

### 1. Use the Windows repo path

Use the repository from the Windows filesystem:

```powershell
cd C:\Users\dmusa\git-repos\vexcoder
```

Avoid the WSL UNC path for the PowerShell-native workflow:

```powershell
\\wsl$\Ubuntu-24.04\home\d\git-repos\vexcoder
```

### 2. Install the Rust toolchain

Download and install the stable MSVC toolchain with `rustup`:

```powershell
$installer = Join-Path $env:TEMP 'rustup-init.exe'
Invoke-WebRequest -Uri 'https://win.rustup.rs/x86_64' -OutFile $installer
& $installer -y --default-toolchain stable
Remove-Item $installer -Force
```

Add Cargo's bin directory to the current shell if needed:

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
cargo --version
rustc --version
```

Persist the Cargo bin directory to the user PATH if your shell does not pick it up automatically.

### 3. Install the MSVC build tools when `link.exe` is missing

If `cargo build` fails with `link.exe not found`, install Visual Studio Build Tools with the C++ workload:

```powershell
$installer = Join-Path $env:TEMP 'vs_BuildTools.exe'
Invoke-WebRequest -Uri 'https://aka.ms/vs/17/release/vs_BuildTools.exe' -OutFile $installer
& $installer --quiet --wait --norestart --nocache --installPath 'C:\BuildTools' --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended
Remove-Item $installer -Force
```

The validated Windows setup in this repo ended up using:

- Visual Studio Build Tools under `C:\BuildTools`
- Windows SDK `10.0.26100.0`
- the MSVC linker at `C:\BuildTools\VC\Tools\MSVC\<version>\bin\Hostx64\x64\link.exe`

If linking fails on `kernel32.lib`, confirm that the Windows SDK libraries exist under:

```powershell
C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0
```

### 4. Install the repo gate tools

Install the tools used by the local format and name-check gates:

```powershell
cargo install ripgrep
cargo install taplo-cli --version 0.8.1
```

Verify them:

```powershell
rg --version
taplo --version
```

### 5. Run the native Windows gate

The PowerShell packaging script can run the native Rust validation path before it packages the archive:

```powershell
.\scripts\release.ps1 -Version v0.1.0-alpha.1 -Target x86_64-pc-windows-msvc -RunGate
```

That runs:

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo check --all-targets`
- `cargo test --all`
- `cargo test --all-targets`

Use the switch when you want CI-equivalent native validation on Windows. Omit it only when you are repackaging an already validated build.

### 6. Build the release binary

The Windows-native release build validated in this session was:

```powershell
cargo build --release --bin vex
```

Run the binary directly:

```powershell
.\target\release\vex.exe
```

### 7. Package a Windows release archive

Use the PowerShell-native packaging script added for this workflow:

```powershell
.\scripts\release.ps1 -Version v0.1.0-alpha.1 -Target x86_64-pc-windows-msvc -RunGate
```

That script automatically:

- discovers the Visual Studio Build Tools install
- discovers the Windows SDK install
- sets `PATH`, `LIB`, `INCLUDE`, and `LIBPATH`
- sets `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER`
- optionally runs the native Rust gate before packaging
- builds the release binary
- writes `dist\vex-<version>-x86_64-pc-windows-msvc.zip`
- writes the matching `.sha256` file

Verify the result:

```powershell
Get-ChildItem dist
Get-FileHash dist\vex-0.1.0-alpha.1-x86_64-pc-windows-msvc.zip -Algorithm SHA256
```

The validated archive produced in this session contained:

- `vex.exe`
- `README.md`
- `LICENSE`

Windows alpha archives are unsigned today. SmartScreen will show an "Unknown Publisher" warning until Authenticode signing is added. SignPath.io is the planned first signing path for open-source release automation.

GitHub Releases publish the same `x86_64-pc-windows-msvc` archive that the native PowerShell packaging flow builds locally.

### 8. Optional GitHub CLI setup

If `gh` is already installed but not on PATH, verify it directly first:

```powershell
& 'C:\Program Files\GitHub CLI\gh.exe' --version
```

Add it to the current shell if needed:

```powershell
$env:Path += ';C:\Program Files\GitHub CLI'
where.exe gh
gh --version
```

Authenticate and confirm the active login:

```powershell
gh auth login
gh auth status
gh api user --jq .login
```

### 9. Install and use mdBook

Install mdBook with Cargo:

```powershell
cargo install mdbook --locked
```

Verify and preview the docs locally:

```powershell
mdbook --version
mdbook build docs
mdbook serve docs --open
```

## Linux source builds

The Linux path is simpler because the repo already uses shell tooling by default.

### 1. Install system prerequisites

On Debian or Ubuntu:

```bash
sudo apt update
sudo apt install -y build-essential curl git pkg-config ca-certificates
```

Install the repo gate tools with the OS package manager or with Cargo. A Cargo-based setup stays consistent across platforms:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
cargo install ripgrep
cargo install taplo-cli --version 0.8.1
cargo install mdbook --locked
```

### 2. Build from source

```bash
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
make gate-fast
cargo build --release --bin vex
./target/release/vex
```

### 3. Optional Linux packaging

To package a local Linux archive with the existing shell workflow:

```bash
bash scripts/release.sh v0.1.0-alpha.1 x86_64-unknown-linux-gnu dist
```

If you need a different target triple, replace the target argument with the host or release target you intend to package.

## macOS source builds

### 1. Install the Apple toolchain and Rust

Install the Xcode command line tools:

```bash
xcode-select --install
```

Then install Rust and the repo helper tools:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
cargo install ripgrep
cargo install taplo-cli --version 0.8.1
cargo install mdbook --locked
```

If you use Homebrew, `gh` is available with:

```bash
brew install gh
```

### 2. Build from source

```bash
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
make gate-fast
cargo build --release --bin vex
./target/release/vex
```

### 3. Optional macOS packaging

For Apple Silicon:

```bash
bash scripts/release.sh v0.1.0-alpha.1 aarch64-apple-darwin dist
```

For Intel macOS:

```bash
bash scripts/release.sh v0.1.0-alpha.1 x86_64-apple-darwin dist
```

## Local docs workflow

The docs site is built with mdBook from `docs/`.

Build the book:

```sh
mdbook build docs
```

Serve it locally with live reload:

```sh
mdbook serve docs --open
```

Generated output is written to `docs/book`.

## GitHub Pages publication

The book is published automatically from `main` by `.github/workflows/docs-build-and-deploy.yml`.

Workflow behavior:

- pull requests build the book and fail on broken docs
- pushes to `main` build the book, upload `docs/book`, and deploy it to GitHub Pages
- local preview still uses `mdbook serve docs --open`

Repository-side requirements:

- Pages source should be set to GitHub Actions
- the `github-pages` environment must be allowed to deploy
- the workflow must keep `pages: write` and `id-token: write`
