#!/usr/bin/env bash
set -euo pipefail

# Keep this check scoped to proprietary/vendor-branded terms and
# external repository-backed identifiers that are disallowed in
# agent/workflow surfaces.
# The caret/editor collision term is intentionally not matched as a
# standalone token because it collides with legitimate variable names
# across the codebase.
#
# Two-pass design:
#   Pass 1 (PATTERN): full pattern across all targets, *excluding* .github/workflows/**
#           Workflow files legitimately reference pinned third-party actions
#           (actions/checkout, dtolnay/rust-toolchain, etc.) via `uses:` directives.
#           Those are not disallowed in CI; they are disallowed in agent/skill surfaces.
#   Pass 2 (BRAND_PATTERN): brand-name-only subset, *including* .github/workflows/**
#           Ensures no proprietary AI brand names appear in workflow YAML even
#           though action-reference patterns are excluded there.
brand_words=(
  $'c\x6c\x61u\x64\x65'
  $'\x61n\x74h\x72o\x70ic'
  $'\x6fpenai'
  $'g\x70t'
  $'c\x6fpilot'
  $'g\x65mini'
  $'c\x6fdewhisperer'
)
brand_regex="$(printf '%s|' "${brand_words[@]}")"
brand_regex="${brand_regex%|}"
caret_host=$'c\x75rsor\\.com'
caret_phrase=$'\\bc\x75rsor ai\\b'
editor_brand=$'\\bVS Code\\b'

PATTERN="\\b(${brand_regex})\\b|${caret_host}|${caret_phrase}|peter-evans/create-pull-request|leonardomso/rust-skills|actions/checkout|actions/cache|actions/upload-pages-artifact|actions/deploy-pages|dtolnay/rust-toolchain|uncenter/setup-taplo|\\bvexcoder/vexcoder\\b|${editor_brand}"

BRAND_PATTERN="\\b(${brand_regex})\\b|${caret_host}|${caret_phrase}|${editor_brand}"

TARGETS=(src .github Makefile)
if [[ -d models ]]; then
  TARGETS+=(models)
fi

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
