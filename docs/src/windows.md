# Windows PowerShell

## 1. Install Git and the Rust toolchain

```powershell
winget install --id Git.Git -e
winget install --id Rustlang.Rustup -e
$env:Path += ";$env:USERPROFILE\\.cargo\\bin"
rustc --version
cargo --version
```

## 2. Clone the repository

```powershell
git clone https://github.com/aistar-au/vexcoder.git
Set-Location vexcoder
```

## 3. Build the release binary

```powershell
cargo build --release
```

## 4. Write the local config files

```powershell
.\target\release\vex.exe init
```

## 5. Write the endpoint config

```powershell
@'
model_url = "http://localhost:8080/v1"
model_name = "local/default"
model_profile = "models/local-balanced.toml"
'@ | Set-Content -Path .vex\config.toml
```

## 6. Export a token only when the endpoint requires one

```powershell
$env:VEX_MODEL_TOKEN = "your-token"
```

## 7. Start the interactive binary

```powershell
.\target\release\vex.exe
```

## 8. Run one non-interactive task

```powershell
.\target\release\vex.exe exec --task "review src/app.rs" --format jsonl
```