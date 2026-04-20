# Linux

## 1. Install the Rust toolchain

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustc --version
cargo --version
```

## 2. Clone the repository

```bash
git clone https://github.com/aistar-au/vexcoder.git
cd vexcoder
```

## 3. Build the release binary

```bash
cargo build --release
```

## 4. Write the local config files

```bash
./target/release/vex init
```

## 5. Write the endpoint config

```bash
cat > .vex/config.toml <<'EOF'
model_url = "http://localhost:8080/v1"
model_name = "local/default"
model_profile = "models/local-balanced.toml"
EOF
```

## 6. Export a token only when the endpoint requires one

```bash
export VEX_MODEL_TOKEN="your-token"
```

## 7. Start the interactive binary

```bash
./target/release/vex
```

## 8. Run one non-interactive task

```bash
./target/release/vex exec --task "review src/app.rs" --format jsonl
```