#!/usr/bin/env bash
set -euo pipefail

# Keep this check scoped to proprietary/vendor-branded terms and
# external repository-backed identifiers that are disallowed in
# agent/workflow surfaces.
# "cursor" is intentionally not matched as a standalone token because it
# collides with legitimate editor/caret variable names across the codebase.
PATTERN='\b(claude|anthropic|openai|gpt|copilot|gemini|codewhisperer)\b|cursor\.com|\bcursor ai\b|peter-evans/create-pull-request|leonardomso/rust-skills|actions/checkout|actions/cache|actions/upload-pages-artifact|actions/deploy-pages|dtolnay/rust-toolchain|uncenter/setup-taplo|\bvexcoder/vexcoder\b|\bVS Code\b'

TARGETS=(src .github .agents docs/book.toml Makefile)

if rg -n --hidden -i --glob '!.git' "$PATTERN" "${TARGETS[@]}"; then
  echo "FAIL: forbidden branded names found in ${TARGETS[*]}"
  exit 1
fi

echo "clean"
