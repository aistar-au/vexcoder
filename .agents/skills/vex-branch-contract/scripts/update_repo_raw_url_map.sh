#!/usr/bin/env bash
# update_repo_raw_url_map.sh
# Keep TASKS/completed/REPO-RAW-URL-MAP.md synchronized with tracked files.
#
# Policy:
# - Default mode updates the map only when tracked files are missing from the map.
# - --check verifies that no tracked files are missing and exits non-zero on mismatch.
# - --force regenerates the map even when nothing is missing.
#
# Usage:
#   bash .agents/skills/vex-branch-contract/scripts/update_repo_raw_url_map.sh
#   bash .agents/skills/vex-branch-contract/scripts/update_repo_raw_url_map.sh --check
#   bash .agents/skills/vex-branch-contract/scripts/update_repo_raw_url_map.sh --force
set -euo pipefail
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=_lib.sh
source "$SCRIPT_DIR/_lib.sh"

map_file="TASKS/completed/REPO-RAW-URL-MAP.md"
repo_slug=""
mode="update"
force="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --map) map_file="$2"; shift 2;;
    --repo-slug) repo_slug="$2"; shift 2;;
    --check) mode="check"; shift 1;;
    --force) force="true"; shift 1;;
    *) die "unknown arg: $1";;
  esac
done

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "not in a git repo"

if [[ -z "$repo_slug" ]]; then
  repo_slug="$(repo_slug_from_origin)"
fi

tmp_dir="$(mktemp -d /tmp/vex-raw-map.XXXXXX)"
tracked="$tmp_dir/tracked.txt"
mapped="$tmp_dir/mapped.txt"
missing="$tmp_dir/missing.txt"
trap 'rm -rf "$tmp_dir"' EXIT

git ls-files | sort -u > "$tracked"

if [[ -f "$map_file" ]]; then
  awk -F'`' '/^\| [0-9]+ \| `/{print $2}' "$map_file" | sort -u > "$mapped"
else
  : > "$mapped"
fi

comm -23 "$tracked" "$mapped" > "$missing"
missing_count="$(wc -l < "$missing" | tr -d ' ')"
tracked_count="$(wc -l < "$tracked" | tr -d ' ')"

if [[ "$mode" == "check" ]]; then
  if [[ "$missing_count" -eq 0 ]]; then
    echo "PASS: $map_file covers all tracked files ($tracked_count entries)."
    exit 0
  fi
  echo "FAIL: $map_file is missing $missing_count tracked file(s):" >&2
  sed 's/^/- /' "$missing" >&2
  exit 1
fi

if [[ "$force" != "true" && "$missing_count" -eq 0 ]]; then
  echo "No new tracked files missing from $map_file. No update needed."
  exit 0
fi

{
  echo "# Repository Raw URL Map"
  echo
  echo "Canonical raw URL index for every tracked file in this repository."
  echo
  echo "- Branch: main"
  echo "- Base: <https://raw.githubusercontent.com/$repo_slug/main/>"
  echo "- Source: git ls-files"
  echo "- Total tracked files: $tracked_count"
  echo
  echo "| # | Path | Approx. lines | Raw URL |"
  echo "| ---: | :--- | ---: | :--- |"
  i=1
  while IFS= read -r f; do
    lines="$(wc -l < "$f" | tr -d ' ')"
    printf '| %d | `%s` | ~%s | <https://raw.githubusercontent.com/%s/main/%s> |\n' \
      "$i" "$f" "$lines" "$repo_slug" "$f"
    i=$((i + 1))
  done < "$tracked"
} > "$map_file"

echo "Updated $map_file with $tracked_count tracked file(s)."
if [[ "$missing_count" -gt 0 ]]; then
  echo "Newly covered file(s):"
  sed 's/^/- /' "$missing"
fi
