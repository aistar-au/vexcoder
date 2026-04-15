#!/usr/bin/env bash
# upgrade-deps.sh — stale-dependency audit and manifest-upgrade helper.
#
# This repository keeps direct dependency requirements in the root
# [workspace.dependencies] table. `cargo upgrade` edits those requirements,
# while `cargo update` refreshes Cargo.lock after a manifest change.

set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  bash scripts/upgrade-deps.sh audit
  bash scripts/upgrade-deps.sh plan [cargo-upgrade args...]
  bash scripts/upgrade-deps.sh apply [cargo-upgrade args...]

Commands:
  audit   Report stale direct workspace dependencies with cargo outdated.
  plan    Dry-run a manifest update with cargo upgrade.
  apply   Update Cargo.toml with cargo upgrade, refresh Cargo.lock, and run cargo check.

Examples:
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
  - TUI stack: src/ui/tui.rs, src/tui_handle.rs
  - XML tool-call parsing: src/state/conversation/tool_call_parser.rs
  - Structural indexing: src/tools/index.rs
  - Markdown rendering: src/ui/render/markdown.rs
  - HTTP and MCP stack: src/api/client/mod.rs, src/mcp.rs, src/server/

Use `cargo tree -i <crate>` to inspect reverse dependencies for one crate.
Use `cargo tree -d` for investigation only; it is not a hard gate because the
transitive tree legitimately carries parallel versions in some ecosystems.
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
  audit)
    if [ "$#" -ne 0 ]; then
      usage
      exit 1
    fi
    require_tool cargo-outdated "cargo install cargo-outdated --locked"
    cargo outdated --workspace --root-deps-only --manifest-path Cargo.toml
    ;;
  plan)
    require_tool cargo-upgrade "cargo install cargo-edit --locked --no-default-features --features upgrade"
    cargo upgrade --manifest-path Cargo.toml --dry-run "$@"
    print_review_seams
    ;;
  apply)
    require_tool cargo-upgrade "cargo install cargo-edit --locked --no-default-features --features upgrade"
    cargo upgrade --manifest-path Cargo.toml "$@"
    cargo update
    cargo check --all-targets
    print_review_seams
    cat <<'EOF'

Next steps:
  cargo fmt --check
  cargo clippy --all-targets -- -D warnings
  cargo nextest run -j 2
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