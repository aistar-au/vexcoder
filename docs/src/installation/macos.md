# Build from Source — macOS

Tested on macOS 12 Monterey and later, both Intel and Apple Silicon.

## Step 1 — Install Xcode Command Line Tools

The Xcode CLT provides `git`, `make`, `clang`, and the macOS SDK headers. If you already have Xcode or the CLT installed this step is a no-op.

```bash
xcode-select --install
```

A dialog will appear. Click Install and wait for the download to complete. Verify:

```bash
xcode-select -p
# expected: /Library/Developer/CommandLineTools or /Applications/Xcode.app/Contents/Developer
```

## Step 2 — Install Homebrew (optional but recommended)

Homebrew provides `taplo` and other tooling used by the Makefile targets. Skip this step if you manage those tools another way.

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

Follow the post-install instructions to add Homebrew to your shell path. On Apple Silicon:

```bash
echo 'eval "$(/opt/homebrew/bin/brew shellenv)"' >> ~/.zprofile
eval "$(/opt/homebrew/bin/brew shellenv)"
```

Verify:

```bash
brew --version
```

## Step 3 — Install Rust via rustup

Do not use the Homebrew Rust package. The project requires the stable toolchain managed by rustup so that toolchain pinning and component installation work correctly.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Choose option 1 (default installation). Then reload your shell environment:

```bash
source "$HOME/.cargo/env"
```

Verify:

```bash
rustup show
cargo --version
```

The project targets stable Rust. Confirm you have at least 1.75:

```bash
rustup update stable
```

## Step 4 — Install taplo (TOML formatter and validator)

The `make gate-fast` target uses `scripts/taplo_safe.sh` to validate TOML files. taplo is available via Homebrew:

```bash
brew install taplo
```

Or via cargo:

```bash
cargo install taplo-cli --locked
```

Verify:

```bash
taplo --version
```

## Step 5 — Clone and build

```bash
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
```

Run the full gate first to confirm the toolchain is set up correctly:

```bash
make gate-fast
```

This runs `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, and `cargo test --all-targets`. A green gate confirms the build environment is complete.

Then build the release binary:

```bash
cargo build --release
```

The binary is at `target/release/vex`.

## Step 6 — Run

```bash
VEX_MODEL_URL=http://localhost:8000/v1/messages \
./target/release/vex
```

Or install to your Cargo bin path:

```bash
cargo install --path .
vex
```

## Step 7 — (Optional) Build a release archive

To produce the same `.tar.gz` archive used by GitHub Releases:

```bash
./scripts/release.sh v0.1.0-alpha.1 x86_64-apple-darwin
```

The archive is written to the `dist/` directory.

## Troubleshooting

**`error: linker 'cc' not found`** — The Xcode CLT installation did not complete or the path was not picked up. Run `xcode-select --install` again or set `CC=clang` in your environment.

**`taplo: command not found`** — Install taplo via Homebrew or cargo (Step 4). The gate will fail if taplo is absent.

**`cargo fmt --check` fails** — Run `cargo fmt` and commit the result before re-running the gate.
