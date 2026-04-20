# macOS

This guide gets the repo-local `vex` binary running on macOS.

Before you start:

- use a checkout where you can write `.vex/` and `AGENTS.md`; `vex init` creates `.vex/config.toml`, `.vex/validate.toml`, and `AGENTS.md`
- `vex` needs an external model endpoint; it does not start a model server for you
- local and private-network endpoints do not need `VEX_MODEL_TOKEN`; remote public endpoints do

## 1. Install Apple command line tools

```bash
xcode-select --install
```

Wait for the installer to finish before continuing.

## 2. Install Rust and verify the toolchain

```bash
git --version
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustc --version
cargo --version
```

## 3. Clone the repository

```bash
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
```

## 4. Build the release binary

```bash
cargo build --release
```

## 5. Scaffold the repo-local workspace files

```bash
./target/release/vex init
```

## 6. Point `vex` at a local server

The fastest local setup is a server that listens on `http://127.0.0.1:8080/v1`.

```bash
cat > .vex/config.toml <<'EOF'
model_url = "http://127.0.0.1:8080/v1"
model_name = "local/default"
model_profile = "models/local-balanced.toml"
EOF
```

If you prefer scheme-and-host discovery instead of a fixed `model_url`, see [Configuration](configuration.md) and set `api_client.base_url` instead.

## 7. Start your local server in another shell session

Make sure it is already listening on `127.0.0.1:8080` before the next step. If your server uses a different host or port, update `model_url` to match.

## 8. Verify the endpoint and permissions

`vex` needs normal user-level write access in this checkout and network access to `model_url`. No extra privileges are required after the toolchain install completes.

```bash
./target/release/vex doctor
```

Expect `VEX_MODEL_URL set` to pass. `Model endpoint reachable` should pass once your server is listening; if it warns, start your local server or update `model_url`, then rerun `vex doctor`.

## 9. Remote endpoints require HTTPS and a token

If you switch away from the local-server path, set `model_url` to the exact `https://` endpoint your server exposes, set `model_name` to a model that endpoint accepts, and then export the token before rerunning `vex doctor`.

```bash
export VEX_MODEL_TOKEN="your-token"
```

## 10. Send one request and open the interactive app

```bash
./target/release/vex --print "Reply with the single word ok."
./target/release/vex
```