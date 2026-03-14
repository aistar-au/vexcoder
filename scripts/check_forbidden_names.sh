#!/usr/bin/env bash
set -euo pipefail

RG_BIN="${VEX_RG_BIN:-rg}"
if [[ "$RG_BIN" == *"/"* || "$RG_BIN" == *"\\"* ]]; then
  if [[ ! -x "$RG_BIN" ]]; then
    echo "FAIL: ripgrep executable not found at $RG_BIN" >&2
    exit 1
  fi
elif ! command -v "$RG_BIN" >/dev/null 2>&1; then
  if command -v rg.exe >/dev/null 2>&1; then
    RG_BIN="rg.exe"
  else
    echo "FAIL: ripgrep executable not found (expected rg, rg.exe, or VEX_RG_BIN)" >&2
    exit 1
  fi
fi

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
#           Ensures no proprietary assistant-brand names appear in workflow YAML even
#           though action-reference patterns are excluded there.
#   Pass 3 (PATH_PATTERN): brand-name subset over prompt/model file paths so
#           branded fixture/template filenames are rejected even when file
#           contents are generic.
brand_words=(
  $'\x63\x6c\x61\x75\x64\x65'
  $'\x61\x6e\x74\x68\x72\x6f\x70\x69\x63'
  $'\x67\x6f\x6f\x67\x6c\x65'
  $'\x6f\x70\x65\x6e\x61\x69'
  $'\x67\x70\x74'
  $'\x63\x6f\x70\x69\x6c\x6f\x74'
  $'\x67\x65\x6d\x69\x6e\x69'
  $'\x71\x77\x65\x6e'
  $'\x64\x65\x65\x70\x73\x65\x65\x6b'
  $'\x63\x6f\x64\x65\x6c\x6c\x61\x6d\x61'
  $'\x73\x74\x61\x72\x63\x6f\x64\x65\x72'
  $'\x63\x6f\x64\x65\x77\x68\x69\x73\x70\x65\x72\x65\x72'
)
brand_regex="$(printf '%s|' "${brand_words[@]}")"
brand_regex="${brand_regex%|}"
caret_host=$'\x63\x75\x72\x73\x6f\x72\\.com'
caret_phrase=$'\\b\x63\x75\x72\x73\x6f\x72 ai\\b'
editor_brand=$'\\bVS Code\\b'

PATTERN="\\b(${brand_regex})\\b|${caret_host}|${caret_phrase}|peter-evans/create-pull-request|leonardomso/rust-skills|actions/checkout|actions/cache|actions/upload-pages-artifact|actions/deploy-pages|dtolnay/rust-toolchain|uncenter/setup-taplo|\\bvexcoder/vexcoder\\b|${editor_brand}"

BRAND_PATTERN="\\b(${brand_regex})\\b|${caret_host}|${caret_phrase}|${editor_brand}"

TARGETS=(
  .gitignore
  AGENTS.md
  CONTRIBUTING.md
  TASKS
  adr
  docs/src
  src
  tests
  scripts
  .github
  Makefile
)
if [[ -d models ]]; then
  TARGETS+=(models)
fi

failed=0

# Pass 1: full pattern — .github/workflows/** excluded (.github non-workflow files still scanned)
if "$RG_BIN" -n --hidden -i \
    --glob '!.git' \
    --glob '!.github/workflows/**' \
    --glob '!scripts/check_forbidden_names.sh' \
    "$PATTERN" "${TARGETS[@]}"; then
  failed=1
fi

# Pass 2: brand names only — also covers .github/workflows/ (no AI brand names in CI YAML)
if [[ -d .github/workflows ]] && \
   "$RG_BIN" -n --hidden -i --glob '!.git' "$BRAND_PATTERN" .github/workflows/; then
  failed=1
fi

# Pass 3: filenames in src/prompts/ and models/ must also stay generic.
for path_root in src/prompts models; do
  if [[ -d "$path_root" ]] && find "$path_root" -type f -print | "$RG_BIN" -n -i "$BRAND_PATTERN"; then
    failed=1
  fi
done

if [[ $failed -ne 0 ]]; then
  echo "FAIL: forbidden branded names found in ${TARGETS[*]}"
  exit 1
fi

echo "clean"
