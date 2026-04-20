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
| 1 | Total Risk | `--force-unstable-alignment` | `-f` | Forces execution even when safety checks are red |
| 2 | Scripting/Print | `--project-map-only` | `-p` | Non-interactive single-turn print to stdout |
| 3 | Context Injection | `--expand-sector-view` | `-e` | Expands file/directory scan for context assembly |
| 4 | Resume Session | `--recall-coordinates` | `-r` | Resumes the saved task at the recorded point |
| 5 | Skip Permissions | `--bypass-integrity-locks` | `-b` | Disables policy enforcement on protected sectors |
| 6 | Plan Mode | `--view-intended-trajectory` | `-v` | Read-only tool policy; previews changes before execution |
| 7 | Model Selection | `--use-alternate-navigator` | `-n` | Overrides the configured model identifier |
| 8 | Verbosity/Debug | `--display-internal-telemetry` | `-d` | Enables RUST_LOG=debug verbose output |
| 9 | Tool Restriction | `--restrict-payload-tools` | `-t` | Restricts tool payload to safe read/search subset |
| 10 | Output Format | `--set-map-encoding` | `-m` | Sets output encoding: json or text |

## Removed

- `--chat-compat`: removed as part of single normalized API consumer cutover.
  Automatic protocol discovery continues to select the appropriate wire
  protocol for non-native endpoints. The explicit CLI override is no longer
  supported.
- `--plan`: superseded by `--view-intended-trajectory` (`-v`).
- `--chat`: superseded by `--restrict-payload-tools` (`-t`).
- `--resume`: superseded by `--recall-coordinates` (`-r`).
- `--print` / `-p`: superseded by `--project-map-only` (`-p`; same short flag,
  same semantics, new canonical name).

## New Config Fields

Three `#[serde(skip)]` fields are added to `Config` to carry CLI-only state:

- `force: bool` — set by `-f/--force-unstable-alignment`
- `bypass_policy: bool` — set by `-b/--bypass-integrity-locks`
- `expand_context: bool` — set by `-e/--expand-sector-view`

These fields are not persisted to TOML. They are wired in `apply_cli_overrides`
after `Config::load()`.

## Compliance

Agents must not reintroduce `--chat-compat` or any CLI flag not in the normative
table above without a further ADR amendment. Protocol selection for non-native
endpoints remains automatic and must not be re-exposed as a user-facing flag.
