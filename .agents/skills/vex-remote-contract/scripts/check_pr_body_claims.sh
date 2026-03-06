#!/usr/bin/env bash
# check_pr_body_claims.sh
#
# String-only preflight for PR body text.
# Blocks known drift-prone phrases that have historically been incorrect
# in agent-drafted PR bodies for aistar-au/vexcoder.
# Run this before any update_pull_request API call.
#
# Usage:
#   echo "$PR_BODY" | bash .agents/skills/vex-remote-contract/scripts/check_pr_body_claims.sh
#   bash .agents/skills/vex-remote-contract/scripts/check_pr_body_claims.sh < pr_body.txt
#
# Exit 0: no blocked phrases found — safe to post.
# Exit 1: one or more blocked phrases found — do not post until resolved.

set -euo pipefail

FAILED=0
INPUT="${1:-/dev/stdin}"

check_phrase() {
  local phrase="$1"
  local reason="$2"
  if grep -qF -- "$phrase" "$INPUT" 2>/dev/null; then
    echo "BLOCKED: \"$phrase\""
    echo "  Reason: $reason"
    echo ""
    FAILED=1
  fi
}

# Trigger scope claims that conflict with current workflow configuration.
# doc-ref-check.yml uses: push: branches: [main] + pull_request.
check_phrase \
  "runs on every push" \
  "doc-ref-check.yml triggers on push to main and pull_request only — not every push to every branch"

check_phrase \
  "on every push and pull request" \
  "doc-ref-check.yml triggers on push to main and pull_request only — not every push"

check_phrase \
  "every push and pull request with no path filter" \
  "doc-ref-check.yml has branches: [main] on push — the no-filter claim is false"

# Source-of-truth claims about update_active_roadmaps.py.
check_phrase \
  "source of truth is ACTIVE-ROADMAP.md" \
  "update_active_roadmaps.py derives status from ADR files for hardcoded IDs — ACTIVE-ROADMAP.md is not the sole source"

check_phrase \
  "ACTIVE-ROADMAP.md as the source of truth" \
  "update_active_roadmaps.py derives status from ADR files for hardcoded IDs — ACTIVE-ROADMAP.md is not the sole source"

check_phrase \
  "based on ACTIVE-ROADMAP.md as source of truth" \
  "update_active_roadmaps.py derives status from ADR files — ACTIVE-ROADMAP.md is not the sole source"

# Outdated brand-rule carve-out phrasing.
check_phrase \
  "wire protocol identifiers" \
  "brand-rule carve-out uses an explicit exclusion list in both skills; 'wire protocol identifiers' is outdated phrasing not present in current skill text"

if [ "$FAILED" -eq 0 ]; then
  echo "check_pr_body_claims: no blocked phrases found"
fi

exit "$FAILED"
