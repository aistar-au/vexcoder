#!/usr/bin/env bash
# checklist_status.sh — Emit one JSONL line per EL-* checklist item from ADR-023.
#
# Output format (one JSON object per line):
#   {"id":"EL-01","title":"...","status":"pending","gate":"...","anchors":["...",...]}
#
# Usage:
#   scripts/checklist_status.sh
#   scripts/checklist_status.sh | jq -r 'select(.status=="pending") | .id' | head -1
#   scripts/checklist_status.sh | jq -r 'select(.id=="EL-01") | .anchors[]'
#
# Exit: always 0. Parsing errors go to stderr; malformed items are skipped.
set -euo pipefail

ADR_FILE="docs/adr/ADR-023-deterministic-edit-loop.md"

if [[ ! -f "$ADR_FILE" ]]; then
  echo "checklist_status.sh: ERROR: $ADR_FILE not found" >&2
  exit 0
fi

# ---------------------------------------------------------------------------
# Pass 1: parse the dispatcher checklist table.
#
# Table header:  | ID | Task | Gate | Status |
# Data rows:     | **EL-NN** | <title> | <gate> | [ ] or [x] |
#
# We only emit a row when we see an EL-* ID in field 2.
# ---------------------------------------------------------------------------

declare -A TITLES
declare -A GATES
declare -A STATUSES

while IFS= read -r line; do
  # Must start with a pipe and contain EL- in a bold marker.
  [[ "$line" =~ ^\|[[:space:]]*\*\*EL-[0-9]+ ]] || continue

  # Extract fields by splitting on '|' using awk.
  id="$(echo "$line" | awk -F'|' '{print $2}')"
  title="$(echo "$line" | awk -F'|' '{print $3}')"
  gate="$(echo "$line" | awk -F'|' '{print $4}')"
  status_raw="$(echo "$line" | awk -F'|' '{print $5}')"

  # Strip leading/trailing whitespace and bold markers from id.
  id="$(echo "$id" | sed 's/[[:space:]]*//g; s/\*//g')"
  title="$(echo "$title" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"
  gate="$(echo "$gate" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"

  [[ "$id" =~ ^EL-[0-9]+$ ]] || continue

  # Status: [x] = done, [ ] = pending.
  if echo "$status_raw" | grep -q '\[x\]'; then
    STATUSES["$id"]="done"
  else
    STATUSES["$id"]="pending"
  fi

  TITLES["$id"]="$title"
  GATES["$id"]="$gate"
done < "$ADR_FILE"

# ---------------------------------------------------------------------------
# Pass 2: parse the sequence table for anchor tests.
#
# Table header:  | Task | Scope | Anchor test | Gate |
# Data rows:     | EL-NN | <scope> | <anchors> | <gate> |
#
# Anchors are semicolon-separated, optionally backtick-wrapped.
# ---------------------------------------------------------------------------

declare -A ANCHORS

in_sequence_table=0

while IFS= read -r line; do
  # Detect entry into the sequence table by its header.
  if echo "$line" | grep -q "Anchor test"; then
    in_sequence_table=1
    continue
  fi

  # Exit the table on a blank line or a new heading.
  if [[ $in_sequence_table -eq 1 ]]; then
    if [[ -z "${line// /}" ]] || [[ "$line" =~ ^# ]]; then
      in_sequence_table=0
      continue
    fi
  fi

  [[ $in_sequence_table -eq 1 ]] || continue
  [[ "$line" =~ ^\| ]] || continue

  id="$(echo "$line" | awk -F'|' '{print $2}' | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"
  anchor_raw="$(echo "$line" | awk -F'|' '{print $4}' | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"

  [[ "$id" =~ ^EL-[0-9]+$ ]] || continue
  [[ -z "$anchor_raw" ]] && continue

  # Remove backticks, then split on semicolons to build a JSON array.
  anchor_raw="$(echo "$anchor_raw" | sed 's/`//g')"

  json_array="["
  first=1
  while IFS= read -r -d ';' token; do
    token="$(echo "$token" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"
    [[ -z "$token" ]] && continue
    if [[ $first -eq 1 ]]; then
      json_array+="\"$token\""
      first=0
    else
      json_array+=",\"$token\""
    fi
  done <<< "${anchor_raw};"

  json_array+="]"
  ANCHORS["$id"]="$json_array"
done < "$ADR_FILE"

# ---------------------------------------------------------------------------
# Emit JSONL — one line per EL-* item in numeric order.
# ---------------------------------------------------------------------------

for id in $(echo "${!STATUSES[@]}" | tr ' ' '\n' | sort -t- -k2 -n); do
  title="${TITLES[$id]:-}"
  gate="${GATES[$id]:-}"
  status="${STATUSES[$id]:-pending}"
  anchors="${ANCHORS[$id]:-[]}"

  # Escape double quotes in title and gate for valid JSON.
  title="$(echo "$title" | sed 's/"/\\"/g')"
  gate="$(echo "$gate" | sed 's/"/\\"/g')"

  printf '{"id":"%s","title":"%s","status":"%s","gate":"%s","anchors":%s}\n' \
    "$id" "$title" "$status" "$gate" "$anchors"
done
