#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${VEXCODER_ROOT:-}" ]]; then
  ROOT="$VEXCODER_ROOT"
elif [[ -f "$PWD/Cargo.toml" && -d "$PWD/docs/src" ]]; then
  ROOT="$PWD"
elif [[ -d "/Users/d/git-repo/vexcoder" ]]; then
  ROOT="/Users/d/git-repo/vexcoder"
else
  echo "ERROR: set VEXCODER_ROOT or run from a vexcoder checkout." >&2
  exit 1
fi

cd "$ROOT"
mdbook build docs
