#!/usr/bin/env bash
# upgrade-deps.sh — stale-dependency audit, security check, and manifest-upgrade helper.
#
# This repository keeps direct dependency requirements in the root
# [workspace.dependencies] table. `cargo upgrade` edits those requirements,
# while `cargo update` refreshes Cargo.lock after a manifest change.
#
# Three complementary tools cover the dependency lifecycle:
#   cargo outdated  — reports which direct requirements have newer versions
#   cargo upgrade   — edits Cargo.toml requirement strings (audit lane)
#   cargo deny      — checks RustSec advisories, licenses, and graph quality
#
# Standard upgrade flow:
#   1. bash scripts/upgrade-deps.sh deny     # security + license gate first
#   2. bash scripts/upgrade-deps.sh audit    # find stale direct deps
#   3. bash scripts/upgrade-deps.sh plan ... # preview manifest edit
#   4. bash scripts/upgrade-deps.sh apply ...# write manifest, update lock, check

set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  bash scripts/upgrade-deps.sh deny  [cargo-deny args...]
  bash scripts/upgrade-deps.sh audit
  bash scripts/upgrade-deps.sh plan  [cargo-upgrade args...]
  bash scripts/upgrade-deps.sh apply [cargo-upgrade args...]

Commands:
  deny    Run cargo-deny: advisories, bans, licenses, and source checks.
  audit   Detect unused deps (cargo-machete) and stale versions (cargo-outdated).
  plan    Dry-run a manifest update with cargo upgrade.
  apply   Upgrade manifest, refresh lock, check, and semver-check library APIs.

Examples:
  bash scripts/upgrade-deps.sh deny
  bash scripts/upgrade-deps.sh deny advisories
  bash scripts/upgrade-deps.sh audit
  bash scripts/upgrade-deps.sh plan -p quick-xml@0.40 -p tree-sitter@0.27
  bash scripts/upgrade-deps.sh apply -p quick-xml@0.40 -p tree-sitter@0.27
  bash scripts/upgrade-deps.sh apply --incompatible allow
EOF
}

require_tool() {
  local binary="$1"
  local install_cmd="$2"
  if ! command -v "$binary" >/dev/null 2>&1; then
    echo ""
    echo "MISSING TOOL: $binary"
    echo "  Install: $install_cmd"
    echo ""
    exit 1
  fi
}

print_review_seams() {
  cat <<'EOF'

Review these local seams when an upgraded crate needs source changes:
  - ratatui / crossterm: src/ui/tui.rs, src/tui_handle.rs,
    src/tui_frontend.rs, src/ui/editor/mod.rs, src/app.rs,
    src/app/overlay.rs, src/app/tests/mod.rs
  - rmcp: src/mcp.rs
  - http: src/http_facade.rs, src/mcp.rs, src/server/http.rs,
    src/server/util.rs, src/server/handlers/mod.rs,
    src/server/handlers/session.rs, src/server/tests.rs
  - tokio: src/runtime/tokio.rs plus the focused runtime, server, and test
    sites listed under [workspace.metadata.upgrade-seams] in Cargo.toml
  - arboard: src/clipboard.rs, src/app/commands/session.rs
  - serde: derives and #[serde(...)] attributes stay next to their owning
    types; review src/** and crates/vexapi-types/src/lib.rs directly
  - rust / MSRV: update package.rust-version and CI toolchain declarations
    together
  - quick-xml: src/state/conversation/tool_call_parser.rs
  - tree-sitter: src/tools/index.rs
  - Markdown rendering: src/ui/render/markdown.rs

Use `cargo tree -i <crate>` to inspect reverse dependencies for one crate.
Use `cargo tree -d` for investigation only; it is not a hard gate because the
transitive tree legitimately carries parallel versions in some ecosystems.
The authoritative seam map lives in [workspace.metadata.upgrade-seams] and
[workspace.metadata.upgrade-notes] in the root Cargo.toml.
EOF
}

MODE="${1:-}"
if [ -z "$MODE" ]; then
  usage
  exit 1
fi
shift || true

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

case "$MODE" in
  deny)
    require_tool cargo-deny "cargo install cargo-deny --locked"
    # Default: check all four categories. Pass explicit categories to scope
    # the check: e.g. 'bash scripts/upgrade-deps.sh deny advisories'
    if [ "$#" -eq 0 ]; then
      cargo deny check advisories bans licenses sources
    else
      cargo deny check "$@"
    fi
    ;;
  audit)
    if [ "$#" -ne 0 ]; then
      usage
      exit 1
    fi
    require_tool cargo-machete "cargo install cargo-machete --locked"
    require_tool cargo-outdated "cargo install cargo-outdated --locked"
    echo "==> Unused-dependency audit (cargo-machete)"
    # cargo-machete exits non-zero when unused deps are found; capture as advisory.
    cargo machete --skip-target-dir 2>&1 || echo "(cargo-machete found potentially unused dependencies — see output above)"
    echo ""
    echo "==> Stale-version audit (cargo-outdated)"
    cargo outdated --workspace --root-deps-only --manifest-path Cargo.toml
    ;;
  plan)
    require_tool cargo-upgrade "cargo install cargo-edit --locked --no-default-features --features upgrade"
    cargo upgrade --manifest-path Cargo.toml --dry-run "$@"
    print_review_seams
    ;;
  apply)
    require_tool cargo-upgrade "cargo install cargo-edit --locked --no-default-features --features upgrade"
    require_tool cargo-semver-checks "cargo install cargo-semver-checks --locked"
    cargo upgrade --manifest-path Cargo.toml "$@"
    cargo update
    cargo check --all-targets
    echo ""
    echo "==> Semver-compatibility check (cargo-semver-checks)"
    # Compares library crate APIs against origin/main. Advisory: exits non-zero on breakage.
    cargo semver-checks check --workspace --baseline-rev origin/main 2>&1 \
      || echo "(cargo-semver-checks: potential API regression detected — review before merging)"
    print_review_seams
    cat <<'EOF'

Next steps:
  bash scripts/upgrade-deps.sh deny   # re-run security + license gate
  cargo fmt --check
  cargo clippy --all-targets -- -D warnings
  cargo nextest run
  cargo test --all-targets
  bash scripts/check_forbidden_names.sh
EOF
    ;;
  help|-h|--help)
    usage
    ;;
  *)
    echo "Unknown mode: $MODE"
    echo ""
    usage
    exit 1
    ;;
esac