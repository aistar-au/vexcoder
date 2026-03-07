#!/usr/bin/env bash
set -euo pipefail

# Keep this check scoped to proprietary/vendor-branded terms and
# external repository-backed identifiers that are disallowed in
# agent/workflow surfaces.
# "cursor" is intentionally not matched as a standalone token because it
# collides with legitimate editor/caret variable names across the codebase.
#
# Two-pass design:
#   Pass 1 (PATTERN): full pattern across all targets, *excluding* .github/workflows/**
#           Workflow files legitimately reference pinned third-party actions
#           (actions/checkout, dtolnay/rust-toolchain, etc.) via `uses:` directives.
#           Those are not disallowed in CI; they are disallowed in agent/skill surfaces.
#   Pass 2 (BRAND_PATTERN): brand-name-only subset, *including* .github/workflows/**
#           Ensures no proprietary AI brand names (claude, anthropic, openai…) appear
#           in workflow YAML even though action-reference patterns are excluded there.
PATTERN='\b(claude|anthropic|openai|gpt|copilot|gemini|codewhisperer)\b|cursor\.com|\bcursor ai\b|peter-evans/create-pull-request|leonardomso/rust-skills|actions/checkout|actions/cache|actions/upload-pages-artifact|actions/deploy-pages|dtolnay/rust-toolchain|uncenter/setup-taplo|\bvexcoder/vexcoder\b|\bVS Code\b'

BRAND_PATTERN='\b(claude|anthropic|openai|gpt|copilot|gemini|codewhisperer)\b|cursor\.com|\bcursor ai\b|\bVS Code\b'

TARGETS=(src .github .agents docs/book.toml Makefile)

failed=0

# Pass 1: full pattern — .github/workflows/** excluded (.github non-workflow files still scanned)
if rg -n --hidden -i \
    --glob '!.git' \
    --glob '!.github/workflows/**' \
    "$PATTERN" "${TARGETS[@]}"; then
  failed=1
fi

# Pass 2: brand names only — also covers .github/workflows/ (no AI brand names in CI YAML)
if [[ -d .github/workflows ]] && \
   rg -n --hidden -i --glob '!.git' "$BRAND_PATTERN" .github/workflows/; then
  failed=1
fi

if [[ $failed -ne 0 ]]; then
  echo "FAIL: forbidden branded names found in ${TARGETS[*]}"
  exit 1
fi

echo "clean"
