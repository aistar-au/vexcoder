# Windows PowerShell

This guide gets the repo-local `vex.exe` binary running from PowerShell.

Before you start:

- use a checkout you can write to; `vex init` creates `.vex\config.toml`, `.vex\validate.toml`, and `AGENTS.md`
- `vex` needs an external model endpoint; it does not start a model server for you
- local and private-network endpoints do not need `VEX_MODEL_TOKEN`; remote public endpoints do

## 1. Install Git and the Rust toolchain

```powershell
winget install --id Git.Git -e
winget install --id Rustlang.Rustup -e
$env:Path += ";$env:USERPROFILE\\.cargo\\bin"
git --version
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

## 4. Scaffold the repo-local workspace files

```powershell
.\target\release\vex.exe init
```

## 5. Point `vex` at a local server

The fastest local setup is a server that listens on `http://127.0.0.1:8080/v1`.

```powershell
@'
model_url = "http://127.0.0.1:8080/v1"
model_name = "local/default"
model_profile = "models/local-balanced.toml"
'@ | Set-Content -Path .vex\config.toml
```

## 6. Start your local server in another PowerShell window

Make sure it is already listening on `127.0.0.1:8080` before the next step. If your server uses a different host or port, update `model_url` to match.

## 7. Verify the endpoint and permissions

`vex` needs normal user-level write access in this checkout and network access to `model_url`. No extra privileges are required after the toolchain install completes.

```powershell
.\target\release\vex.exe doctor
```

Expect `VEX_MODEL_URL set` and `Model endpoint reachable` to pass. If the endpoint check warns, start your local server or update `model_url`.

## 8. Remote endpoints require HTTPS and a token

If you switch away from the local-server path, set `model_url` to the exact `https://` endpoint your server exposes, set `model_name` to a model that endpoint accepts, and then export the token before rerunning `vex doctor`.

```powershell
$env:VEX_MODEL_TOKEN = "your-token"
```

## 9. Send one request and open the interactive app

```powershell
.\target\release\vex.exe --print "Reply with the single word ok."
.\target\release\vex.exe
```