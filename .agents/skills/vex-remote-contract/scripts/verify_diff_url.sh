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
fetched_via=""

# Primary fetch — curl with explicit HTTP status check (--fail is not used so we can
# distinguish network errors from auth/404 errors and give a useful message).
# BUG-1 fix: capture HTTP code and check it explicitly; do NOT rely on curl exit code alone.
# BUG-3 fix: gh api fallback when curl fails or returns non-200.
if command -v curl >/dev/null 2>&1; then
  diff_code="$(curl -L -sS --max-time "$timeout" -o "$tmp" -w '%{http_code}' "$diff_url" || true)"
  if [[ "$diff_code" == "200" ]]; then
    fetched_via="curl"
  fi
fi

if [[ -z "$fetched_via" ]]; then
  # wget fallback — useful on minimal Linux images where curl is absent.
  if command -v wget >/dev/null 2>&1; then
    if wget -q --timeout="$timeout" -O "$tmp" "$diff_url" 2>/dev/null; then
      fetched_via="wget"
    fi
  fi
fi

if [[ -z "$fetched_via" ]]; then
  # gh api compare fallback — works when raw CDN is firewalled but API is accessible.
  # Extracts owner/repo and branch names from the diff URL for the API call.
  if command -v gh >/dev/null 2>&1; then
    # diff URL forms:
    #   https://github.com/<owner>/<repo>/compare/<base>...<head>.diff
    #   https://patch-diff.githubusercontent.com/raw/<owner>/<repo>/pull/<N>.diff
    if [[ "$diff_url" =~ github\.com/([^/]+/[^/]+)/compare/([^.]+)\.\.\.([^.]+)\.diff ]]; then
      gh_slug="${BASH_REMATCH[1]}"
      gh_base="${BASH_REMATCH[2]}"
      gh_head="${BASH_REMATCH[3]}"
      if gh api "repos/$gh_slug/compare/${gh_base}...${gh_head}" \
          -H "Accept: application/vnd.github.diff" \
          --jq '.' > "$tmp" 2>/dev/null || \
         gh api "repos/$gh_slug/compare/${gh_base}...${gh_head}.diff" \
          > "$tmp" 2>/dev/null; then
        fetched_via="gh-api"
      fi
    fi
  fi
fi

[[ -n "$fetched_via" ]] \
  || die "failed to fetch diff from $diff_url (tried curl, wget, gh api — all failed or unavailable)"

mapfile -t seen < <(grep -E '^diff --git a/' "$tmp" | sed -E 's/^diff --git a\/([^ ]+).*/\1/' | sort -u)

echo "# Diff URL verification"
echo
echo "- URL: \`$diff_url\`"
echo "- Ref: \`$ref\`"
echo "- Fetched via: \`$fetched_via\`"
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