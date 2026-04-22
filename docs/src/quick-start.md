# Quick Start

This page gets you from clone to a first verified response in the fewest steps.

## 1. Build the binary

```bash
git clone https://github.com/aistar-au/vexapi.git
cd vexapi
cargo build --release
```

The binary will be at `target/release/vex`.

## 2. Create a workspace

```bash
./target/release/vex init
```

This creates:

- `.vex/config.toml`
- `.vex/validate.toml`
- `AGENTS.md`

## 3. Configure your model endpoint

Write `.vex/config.toml` with a reachable endpoint. Local and private-network endpoints can stay on plain HTTP and do not need `VEX_MODEL_TOKEN`. Remote public endpoints must use `https://` and a token.

```bash
cat > .vex/config.toml <<'EOF'
model_url = "http://127.0.0.1:8080/v1"
model_name = "local/default"
model_profile = "models/local-balanced.toml"
EOF
```

If you are connecting to a remote public endpoint, change `model_url` to the exact `https://` URL that endpoint exposes, set `model_name` to a model it accepts, and export the token before continuing:

```bash
export VEX_MODEL_TOKEN="your-token"
```

For scheme-and-host discovery through `api_client.base_url`, see [Configuration](configuration.md).

## 4. Verify the endpoint

```bash
./target/release/vex doctor
```

Expect `VEX_MODEL_URL set` to pass. `Model endpoint reachable` should pass once your server is listening; if it warns, start the server or update `model_url`, then rerun `vex doctor`.

## 5. Start the interactive UI

```bash
./target/release/vex --project-map-only "Reply with the single word ok."
./target/release/vex
```

## 6. Run one-shot or batch commands

One-shot plain text:

```bash
./target/release/vex -p "summarise this repository"
```

Batch mode:

```bash
./target/release/vex exec --task "review src/app.rs" --format jsonl
```

## Next

- [Configuration](configuration.md)
- [CLI and TUI Commands](commands.md)
