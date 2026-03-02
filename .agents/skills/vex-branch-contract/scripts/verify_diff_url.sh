#!/usr/bin/env bash
# verify_diff_url.sh — Confirm a .diff URL contains all expected file paths.
# Usage: verify_diff_url.sh -u <diff-url> [-b <branch>] [--base <ref>] [--timeout <sec>]
set -euo pipefail
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=_lib.sh
source "$SCRIPT_DIR/_lib.sh"

branch=""
diff_url=""
base_ref="origin/main"
timeout="30"

while [[ $# -gt 0 ]]; do
  case "$1" in
    -b|--branch)   branch="$2";   shift 2;;
    -u|--url)      diff_url="$2"; shift 2;;
    --base)        base_ref="$2"; shift 2;;
    --timeout)     timeout="$2";  shift 2;;
    *) die "unknown arg: $1";;
  esac
done

[[ -n "$diff_url" ]] || die "need -u <diff_url>"
git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "not in a git repo"
git fetch origin >/dev/null 2>&1 || die "git fetch origin failed — check network and credentials"
[[ -z "$(git status --porcelain)" ]] || die "working tree not clean"

current_branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
if [[ -z "$branch" ]]; then
  [[ -n "$current_branch" && "$current_branch" != "HEAD" ]] \
    || die "no -b/--branch provided and cannot infer branch from detached HEAD"
  branch="$current_branch"
fi

git ls-remote --exit-code --heads origin "$branch" >/dev/null 2>&1 \
  || die "remote branch missing on origin: $branch"

if [[ -n "$branch" && "$branch" != "$current_branch" ]]; then
  ref="origin/$branch"
else
  ref="HEAD"
fi

mapfile -t expected < <(git diff --name-only "$base_ref...$ref" | sed '/^$/d')
[[ ${#expected[@]} -gt 0 ]] || die "no changes vs $base_ref...$ref"

tmp="/tmp/.vex_diff.$$"
curl -L -sS --max-time "$timeout" -o "$tmp" "$diff_url" || die "failed to fetch diff from $diff_url"

mapfile -t seen < <(grep -E '^diff --git a/' "$tmp" | sed -E 's/^diff --git a\/([^ ]+).*/\1/' | sort -u)

echo "# Diff URL verification"
echo
echo "- URL: \`$diff_url\`"
echo "- Ref: \`$ref\`"
echo "- Expected files: \`${#expected[@]}\`"
echo "- Diff files seen: \`${#seen[@]}\`"
echo

missing=0
for f in "${expected[@]}"; do
  if ! printf "%s\n" "${seen[@]}" | grep -qx "$f"; then
    echo "- [ ] MISSING in diff: \`$f\`"
    missing=1
  else
    echo "- [x] PRESENT: \`$f\`"
  fi
done

rm -f "$tmp" || true

if [[ "$missing" -ne 0 ]]; then
  echo
  die "diff is missing expected paths"
fi

echo
echo "**PASS** — diff contains all expected paths"
