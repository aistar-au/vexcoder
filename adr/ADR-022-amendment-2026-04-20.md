# ADR-022 Amendment — 2026-04-20: Normalized CLI Flag Surface

**Date:** 2026-04-20
**Status:** Amended
**Amends:** ADR-022 (Decision items 1, 7)
**Scope:** CLI flag naming and legacy API protocol removal

## Summary

Replaces the ad-hoc flag surface inherited from the pre-normalization era with
exactly ten semantic flags aligned to the single normalized API consumer path
and the ratatui-native TUI stack. Removes `--chat-compat` and the CLI-level
`ModelProtocol::ChatCompat` override as the ChatCompat alternative routing path
is incompatible with the normalized single-consumer architecture. Protocol
detection for non-native endpoints continues to operate automatically; the
explicit CLI override is removed.

## Normative Flag Table

| # | Intent | Full Flag | Short | Description |
| ---: | :--- | :--- | :--- | :--- |
| 1 | Total Risk | `--force-unstable-alignment` | `-f` | Enables session-wide or task-wide auto-approval |
| 2 | Scripting/Print | `--project-map-only` | `-p` | Non-interactive single-turn print to stdout |
| 3 | Context Injection | `--expand-sector-view` | `-e` | Expands related-path and directory scan limits for context assembly |
| 4 | Resume Session | `--recall-coordinates` | `-r` | Resumes the saved task at the recorded point |
| 5 | Skip Permissions | `--bypass-integrity-locks` | `-b` | Disables runtime durable-state disk-policy enforcement for the process |
| 6 | Plan Mode | `--view-intended-trajectory` | `-v` | Selects the existing read-only planning tool policy |
| 7 | Model Selection | `--use-alternate-navigator` | `-n` | Overrides the configured model identifier |
| 8 | Verbosity/Debug | `--display-internal-telemetry` | `-d` | Enables RUST_LOG=debug verbose output |
| 9 | Tool Restriction | `--restrict-payload-tools` | `-t` | Selects the existing safe read/search tool subset directly |
| 10 | Output Format | `--set-map-encoding` | `-m` | Sets output encoding: jsonl or text |

## Removed

- `--chat-compat`: removed as part of single normalized API consumer cutover.
  Automatic protocol discovery continues to select the appropriate wire
  protocol for non-native endpoints. The explicit CLI override is no longer
  supported.
- `--plan`: superseded by `--view-intended-trajectory` (`-v`).
- `--chat`: no longer exposed as a top-level flag. `ToolPolicy::Chat` remains
  available via `tool_policy = "chat"` in config when plain conversation mode
  is required.
- `--resume`: superseded by `--recall-coordinates` (`-r`).
- `--print` / `-p`: superseded by `--project-map-only` (`-p`; same short flag,
  same semantics, new canonical name).

## New Config Fields

Three `#[serde(skip)]` fields are added to `Config` to carry CLI-only state:

- `force: bool` — enables session-wide or task-wide auto-approval
- `bypass_policy: bool` — overrides runtime durable-state disk-policy mode to `off`
- `expand_context: bool` — raises context-assembly related-path and directory scan caps

These fields are not persisted to TOML. They are wired in `apply_cli_overrides`
after `Config::load()`.

## Subcommand flag normalization (Phase 2)

All per-subcommand optional `--flags` have been removed from the normalized
surface. The following flags are deleted and must not be re-introduced without
a further ADR amendment:

| Subcommand | Removed flags | Replacement |
| :--- | :--- | :--- |
| `exec` | `--task`, `--task-file`, `--max-turns`, `--auto-approve`, `--output`, `--format` | task via `-p`; format via `-m`; auto-approve via `-f` |
| `tasks list` | `--json` | `-m jsonl` |
| `tasks watch` | `--json` | `-m jsonl` |
| `doctor` | `--json` | `-m jsonl` |
| `export` | `--format`, `--output`, `--force` | format via `-m`; output always stdout |
| `serve` | `--host`, `--port` | read from `Config` |
| `init` | `--dir` | use `cd` to target a different directory |
| `credentials set` | `--stdin`, `--from-env` | stdin auto-detected by TTY state |
| `skills install` | `--subdir` | skill root defaults to repository root |

Secret acquisition for `credentials set` now follows automatic source selection:
stdin is consumed when it is not a TTY; an interactive masked prompt is
presented on a full TTY. The secret is never accepted as a positional argument.

## Compliance

Agents must not reintroduce `--chat-compat`, any flag not in the normative
top-level table above, or any subcommand flag listed in the Phase 2 removal
table, without a further ADR amendment. Protocol selection for non-native
endpoints remains automatic and must not be re-exposed as a user-facing flag.
