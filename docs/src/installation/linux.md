# Build from Source — Linux

Tested on Ubuntu 22.04 LTS, Fedora 39, and Arch Linux. The instructions use Debian/Ubuntu package names; substitute your package manager where noted.

## Step 1 — Install system build dependencies

VexCoder's direct Rust dependencies do not require native system libraries beyond a C linker. Install the minimal set:

**Debian / Ubuntu:**

```bash
sudo apt-get update
sudo apt-get install -y build-essential curl git pkg-config
```

**Fedora / RHEL:**

```bash
sudo dnf install -y gcc make curl git pkg-config
```

**Arch Linux:**

```bash
sudo pacman -Sy --needed base-devel curl git
```

Verify the C compiler is available:

```bash
cc --version
```

## Step 2 — Install Rust via rustup

Do not use your distribution's Rust package. System-packaged Rust is often multiple releases behind and does not support toolchain components reliably.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Choose option 1 (default installation). Reload your shell:

```bash
source "$HOME/.cargo/env"
```

Update to the latest stable toolchain:

```bash
rustup update stable
rustup show
cargo --version
```

## Step 3 — (Optional) Add the musl target for static builds

The GitHub Releases Linux binary is compiled against musl for maximum portability. If you want to produce the same fully static binary:

```bash
# Install musl toolchain (Debian/Ubuntu)
sudo apt-get install -y musl-tools

# Add the Rust target
rustup target add x86_64-unknown-linux-musl
```

## Step 4 — Install taplo (TOML formatter and validator)

```bash
cargo install taplo-cli --locked
```

Add `$HOME/.cargo/bin` to your `PATH` if it is not already present:

```bash
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
taplo --version
```

## Step 5 — Clone and build

```bash
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
```

Run the gate:

```bash
make gate-fast
```

Build the standard dynamically-linked release binary:

```bash
cargo build --release
```

Or build the fully static musl binary:

```bash
cargo build --release --target x86_64-unknown-linux-musl
```

Binary paths:

- Dynamic: `target/release/vex`
- Static musl: `target/x86_64-unknown-linux-musl/release/vex`

## Step 6 — Run

```bash
VEX_MODEL_URL=http://localhost:8000/v1/messages \
./target/release/vex
```

Install to your Cargo bin path:

```bash
cargo install --path .
vex
```

## Step 7 — (Optional) Build a release archive

```bash
./scripts/release.sh v0.1.0-alpha.1 x86_64-unknown-linux-musl
```

The archive is written to `dist/`.

## Troubleshooting

**`error: could not find native static library 'c'` when building musl** — The `musl-tools` package is not installed. Run `sudo apt-get install musl-tools` (Debian/Ubuntu) or the equivalent for your distribution.

**`taplo: command not found`** — `$HOME/.cargo/bin` is not on your PATH. Add it to your shell profile (see Step 4).

**`make gate-fast` fails with clippy warnings** — The project compiles with `-D warnings` (warnings as errors). Check the clippy output and fix the flagged items before committing.
