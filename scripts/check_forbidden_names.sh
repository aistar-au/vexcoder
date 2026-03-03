#!/usr/bin/env bash
set -euo pipefail

# Keep this check scoped to proprietary/vendor-branded terms.
# "cursor" is intentionally not matched as a standalone token because it
# collides with legitimate editor/caret variable names across the codebase.
PATTERN='(?i)\b(claude|anthropic|openai|gpt|copilot|gemini|codewhisperer)\b|cursor\.com|\bcursor ai\b'

if rg -n --hidden --pcre2 --glob '!.git' "$PATTERN" src .github; then
  echo "FAIL: forbidden branded names found in src/.github"
  exit 1
fi

echo "clean"
