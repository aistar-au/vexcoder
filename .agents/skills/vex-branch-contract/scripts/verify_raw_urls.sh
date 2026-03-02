#!/usr/bin/env bash
# verify_raw_urls.sh — HTTP-check every changed file's raw GitHub URL.
# With --compare: also SHA-256 compare fetched content vs git ref.
# Usage: verify_raw_urls.sh [-b <branch>] [--base <ref>] [--compare] [--timeout <sec>]
set -euo pipefail
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=_lib.sh
source "$SCRIPT_DIR/_lib.sh"

branch=""
base_ref="origin/main"
compare="false"
timeout="20"

while [[ $# -gt 0 ]]; do
  case "$1" in
    -b|--branch)   branch="$2";   shift 2;;
    --base)        base_ref="$2"; shift 2;;
    --compare)     compare="true"; shift 1;;
    --timeout)     timeout="$2";  shift 2;;
    *) die "unknown arg: $1";;
  esac
done

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "not in a git repo"
git fetch origin >/dev/null 2>&1 || die "git fetch origin failed — check network and credentials"

current_branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
if [[ -z "$branch" ]]; then
  [[ -n "$current_branch" && "$current_branch" != "HEAD" ]] \
    || die "no -b/--branch provided and cannot infer branch from detached HEAD"
  branch="$current_branch"
fi

[[ -z "$(git status --porcelain)" ]] || die "working tree not clean"
git ls-remote --exit-code --heads origin "$branch" >/dev/null 2>&1 \
  || die "remote branch missing on origin: $branch"

if [[ -n "$branch" && "$branch" != "$current_branch" ]]; then
  ref="origin/$branch"
else
  ref="HEAD"
fi

repo_slug="$(repo_slug_from_origin)"

mapfile -t files < <(git diff --name-only "$base_ref...$ref" | sed '/^$/d')
[[ ${#files[@]} -gt 0 ]] || die "no changes vs $base_ref...$ref"

echo "# Raw URL verification: \`$branch\`"
echo
echo "- Repo: \`$repo_slug\`"
echo "- Files: \`${#files[@]}\`"
echo "- Ref: \`$ref\`"
echo "- Mode: \`$([[ "$compare" == "true" ]] && echo "http+content" || echo "http-only")\`"
echo

tmp_remote="/tmp/.vex_raw_tmp.$$"
tmp_git="/tmp/.vex_git_tmp.$$"
fail=0

for f in "${files[@]}"; do
  url="https://raw.githubusercontent.com/$repo_slug/$branch/$f"
  fetched_via=""

  # --- Primary: curl with explicit HTTP code check ---
  if command -v curl >/dev/null 2>&1; then
    code="$(curl -L -sS -o "$tmp_remote" --max-time "$timeout" -w '%{http_code}' "$url" || true)"
    if [[ "$code" == "200" ]]; then
      fetched_via="curl"
    fi
  fi

  # --- Fallback 1: wget (common on minimal Linux images where curl is absent) ---
  if [[ -z "$fetched_via" ]] && command -v wget >/dev/null 2>&1; then
    if wget -q --timeout="$timeout" -O "$tmp_remote" "$url" 2>/dev/null; then
      fetched_via="wget"
    fi
  fi

  # --- Fallback 2: gh api raw-content (when CDN is firewalled but API is reachable) ---
  # BUG-2 fix: pass --timeout to gh api via GH_HTTP_TIMEOUT env var (gh respects it).
  if [[ -z "$fetched_via" ]] && command -v gh >/dev/null 2>&1; then
    if GH_HTTP_TIMEOUT="$timeout" gh api \
        -H "Accept: application/vnd.github.raw" \
        "repos/$repo_slug/contents/$f?ref=$branch" > "$tmp_remote" 2>/dev/null; then
      fetched_via="gh-api"
    fi
  fi

  if [[ -z "$fetched_via" ]]; then
    echo "- [ ] FAIL $f (all fetch methods failed: curl, wget, gh api) — $url"
    fail=1
    continue
  fi

  if [[ "$compare" == "true" ]]; then
    # Compare against content at $ref so this works even when off-branch.
    if ! git show "$ref:$f" > "$tmp_git" 2>/dev/null; then
      echo "- [ ] FAIL $f (cannot read from git ref \`$ref:$f\`) — $url"
      fail=1
      continue
    fi
    git_sha="$(sha256_file "$tmp_git")"
    remote_sha="$(sha256_file "$tmp_remote")"
    if [[ "$git_sha" != "$remote_sha" ]]; then
      echo "- [ ] FAIL $f (content mismatch vs \`$ref\`) — $url"
      echo "  - git($ref): $git_sha"
      echo "  - remote:    $remote_sha"
      fail=1
      continue
    fi
  fi

  case "$fetched_via" in
    curl)   echo "- [x] OK   $f — $url" ;;
    wget)   echo "- [x] OK   $f — $url (via wget)" ;;
    gh-api) echo "- [x] OK   $f — $url (via gh api fallback)" ;;
  esac
done

rm -f "$tmp_remote" "$tmp_git" || true

if [[ "$fail" -ne 0 ]]; then
  echo
  die "raw URL verification failed"
fi

echo
echo "**PASS**"