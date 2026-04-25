# ADR-023: Deterministic Edit Loop

**Status:** Locked  
**Chain:** ADR-020, ADR-022, ADR-016, ADR-006

## Context

No runtime construct drove a coding task from instruction to validated outcome. Missing: bounded pulse cycles, structured retry context on failure, and model-profile loading.

## Decision

- Introduce six additive modules over the existing runtime; none modify `RuntimeMode` or add alternate routing paths.
- `src/prompts/` holds UTF-8 template files loaded via `include_str!`; injected only when edit loop or slash command is active.
- `src/models/` holds `ModelProfile` TOML files: `temperature`, `top_p`, `max_tokens`, `stop_sequences`, `structured_tools`, `reasoning_budget`.
- `ContextAssembler` assembles file snapshots, git metadata, and inferred related paths before each pulse.
- `EditLoop` enforces a bounded cycle (default max 6 pulses, ceiling 12); exits on validation pass or explicit user abort.
- `ValidationSuite` infers test commands from repo structure (Cargo.toml → `cargo test`, package.json → `npm test`, etc.).
- Eight slash commands: `/edit`, `/fix`, `/explain`, `/run`, `/test`, `/review`, `/plan`, `/context`.
- `/review` and `/plan` are read-only, single-pulse; silently drop pending patches on entry.
- `/context` is zero-pulse status display; `/commands` renders from the work table.
- Prompt templates must not contain provider names; CI check (`scripts/check_forbidden_names.sh`) covers `src/prompts/` content.

## References

- [`toml`](https://docs.rs/toml) — profile deserialization
- [`tree-sitter`](https://docs.rs/tree-sitter) — AST parsing for context assembly
- [`tempfile`](https://docs.rs/tempfile) — validation suite scratch files
