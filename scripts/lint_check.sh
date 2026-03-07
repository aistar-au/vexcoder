#!/usr/bin/env bash
# lint_check.sh — Structured clippy output filtered for agent consumption.
#
# Runs cargo clippy --all-targets --message-format json and filters to
# error-level diagnostics only. Exits non-zero when any errors are present.
#
# Usage:
#   scripts/lint_check.sh            # emit JSON error objects to stdout, one per line
#   scripts/lint_check.sh --count    # emit just the error count as an integer
#
# Exit codes:
#   0 — no clippy errors
#   1 — one or more clippy errors found
set -euo pipefail

mode="full"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --count) mode="count"; shift ;;
    *) echo "lint_check.sh: ERROR: unknown argument: $1" >&2; exit 1 ;;
  esac
done

# Run clippy with JSON output. Suppress stderr (user messages, progress) so
# only the structured JSON stream goes to the pipe.
raw="$(cargo clippy --all-targets --message-format json 2>/dev/null || true)"

# Filter to compiler-message records with level == "error".
errors="$(echo "$raw" | awk '
  /"reason":"compiler-message"/ && /"level":"error"/ { print }
')"

count=0
if [[ -n "$errors" ]]; then
  count="$(echo "$errors" | wc -l | tr -d ' ')"
fi

if [[ "$mode" == "count" ]]; then
  echo "$count"
  [[ "$count" -eq 0 ]] && exit 0 || exit 1
fi

if [[ "$count" -eq 0 ]]; then
  echo '{"lint_errors":0}'
  exit 0
fi

echo "$errors"
exit 1
