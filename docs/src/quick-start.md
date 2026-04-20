# Quick Start

This page gets you from clone to a running session in the fewest steps.

## 1. Build the binary

```bash
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
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

Local example:

```toml
# .vex/config.toml
model_url = "http://localhost:8080/v1"
model_name = "local/default"
model_profile = "models/local-balanced.toml"
```

For a local Messages-v1 server, use plain HTTP unless you
have explicitly configured TLS:

```toml
# .vex/config.toml
model_url = "http://localhost:8000/v1/messages"
model_name = "your-model-name"
model_profile = "models/local-balanced.toml"
```

Remote example:

```toml
# .vex/config.toml
model_url = "https://your-endpoint.example/v1/messages"
model_name = "your-model-name"
model_profile = "models/api-structured.toml"
```

Export a token only when the endpoint requires one:

```bash
export VEX_MODEL_TOKEN="your-token"
```

## 4. Start the interactive UI

```bash
./target/release/vex
```

## 5. Run one-shot or batch commands

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
- [Architecture Overview](architecture.md)
