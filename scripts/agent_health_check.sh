#!/usr/bin/env bash
# agent_health_check.sh — Composite health check for ADR-023 dispatcher items.
#
# Implements ADR-024 Gap 27 (make health-check).
#
# Runs:
#   1. make gate-fast          — full local gate minus map-check
#   2. Anchor tests for every "done" checklist item (via checklist_status.sh)
#
# Usage:
#   scripts/agent_health_check.sh              # full health check
#   scripts/agent_health_check.sh --gate-only  # just make gate-fast
#   scripts/agent_health_check.sh EL-01        # single item anchor check
#
# Output:
#   Human-readable summary to stdout.
#   One JSONL result line per completed item: {"id":"EL-01","anchors_run":3,"pass":true}
#
# Exit codes:
#   0 — all checks passed
#   1 — one or more checks failed
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"

die() { echo "agent_health_check.sh: ERROR: $*" >&2; exit 1; }

command -v jq >/dev/null 2>&1 || die "jq is required — install via: brew install jq  OR  apt-get install jq"

mode="full"
single_item=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --gate-only) mode="gate-only"; shift ;;
    EL-*) single_item="$1"; mode="single"; shift ;;
    *) die "unknown argument: $1" ;;
  esac
done

cd "$REPO_ROOT"

# ---------------------------------------------------------------------------
# Step 1 — gate-fast
# ---------------------------------------------------------------------------
echo ""
echo "==> agent_health_check: running make gate-fast"
if make gate-fast; then
  echo "==> gate-fast: PASS"
else
  echo "==> gate-fast: FAIL"
  exit 1
fi

[[ "$mode" == "gate-only" ]] && { echo ""; echo "health-check: gate-only pass"; exit 0; }

# ---------------------------------------------------------------------------
# Step 2 — anchor tests
# ---------------------------------------------------------------------------
CHECKLIST_SCRIPT="$SCRIPT_DIR/checklist_status.sh"
[[ -x "$CHECKLIST_SCRIPT" ]] || die "checklist_status.sh not found or not executable at $CHECKLIST_SCRIPT"

overall_pass=true

run_anchors_for_item() {
  local id="$1"
  local anchors_json="$2"

  local anchors=()
  while IFS= read -r anchor; do
    anchors+=("$anchor")
  done < <(echo "$anchors_json" | jq -r '.[]')

  local count="${#anchors[@]}"
  local item_pass=true

  echo ""
  echo "==> anchor tests for $id ($count test(s))"

  for anchor in "${anchors[@]}"; do
    echo "    cargo test $anchor --all-targets"
    if cargo test "$anchor" --all-targets 2>&1 | tail -5; then
      echo "    PASS: $anchor"
    else
      echo "    FAIL: $anchor"
      item_pass=false
    fi
  done

  if [[ "$item_pass" == "true" ]]; then
    printf '{"id":"%s","anchors_run":%d,"pass":true}\n' "$id" "$count"
  else
    printf '{"id":"%s","anchors_run":%d,"pass":false}\n' "$id" "$count"
    overall_pass=false
  fi
}

if [[ "$mode" == "single" ]]; then
  # Single item mode — find the item and run its anchors regardless of status.
  item_json="$(bash "$CHECKLIST_SCRIPT" | jq -c --arg id "$single_item" 'select(.id == $id)')"
  [[ -n "$item_json" ]] || die "item $single_item not found in checklist"
  anchors_json="$(echo "$item_json" | jq -c '.anchors')"
  run_anchors_for_item "$single_item" "$anchors_json"
else
  # Full mode — run anchors for all "done" items.
  done_items="$(bash "$CHECKLIST_SCRIPT" | jq -c 'select(.status == "done")')"

  if [[ -z "$done_items" ]]; then
    echo ""
    echo "==> no completed checklist items — skipping anchor tests"
  else
    while IFS= read -r item_json; do
      id="$(echo "$item_json" | jq -r '.id')"
      anchors_json="$(echo "$item_json" | jq -c '.anchors')"
      run_anchors_for_item "$id" "$anchors_json"
    done <<< "$done_items"
  fi
fi

echo ""
if [[ "$overall_pass" == "true" ]]; then
  echo "health-check: PASS"
  exit 0
else
  echo "health-check: FAIL — one or more anchor tests failed"
  exit 1
fi
