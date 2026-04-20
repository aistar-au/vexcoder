# Build From Source

`vex` builds as a local CLI binary, but it does not bundle a model runtime.
Before you start, make sure you have:

- Git and a stable Rust toolchain on your `PATH`
- permission to clone into a writable directory and create `.vex/` files plus `AGENTS.md` inside it
- a reachable model endpoint

Use a local server on `http://127.0.0.1:8080/v1` when you want the fastest same-machine setup. Local and private-network endpoints can stay on plain HTTP and do not need `VEX_MODEL_TOKEN`. Use a remote public endpoint only when it is exposed over `https://`; those endpoints require `VEX_MODEL_TOKEN`. For alternative endpoint wiring through `api_client.base_url`, see [Configuration](configuration.md).

Open the page for your shell:

- [macOS](macos.md)
- [Linux](linux.md)
- [Windows PowerShell](windows.md)
