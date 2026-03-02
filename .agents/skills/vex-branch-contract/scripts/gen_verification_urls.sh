#!/usr/bin/env bash
# gen_verification_urls.sh — Generate raw GitHub URL map for a branch.
# Usage: gen_verification_urls.sh [-b <branch>] [-o <out>] [--base <ref>] [--urls-only]
set -euo pipefail
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=_lib.sh
source "$SCRIPT_DIR/_lib.sh"

branch=""
out=""
base_ref="origin/main"
urls_only="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    -b|--branch)   branch="$2";   shift 2;;
    -o|--out)      out="$2";      shift 2;;
    --base)        base_ref="$2"; shift 2;;
    --urls-only)   urls_only="true"; shift 1;;
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

# Ensure remote branch exists.
git ls-remote --exit-code --heads origin "$branch" >/dev/null 2>&1 \
  || die "remote branch missing on origin: $branch"

# Use named branch ref when -b differs from checked-out branch.
if [[ -n "$branch" && "$branch" != "$current_branch" ]]; then
  ref="origin/$branch"
else
  ref="HEAD"
fi

[[ -z "$(git status --porcelain)" ]] || die "working tree not clean"

repo_slug="$(repo_slug_from_origin)"
head_sha="$(git rev-parse "$ref")"
short_sha="$(git rev-parse --short "$ref")"

# Changed files vs base.
mapfile -t files < <(git diff --name-only "$base_ref...$ref" | sed '/^$/d')
[[ ${#files[@]} -gt 0 ]] || die "no changes vs $base_ref...$ref (did you pick the right base/ref?)"

if [[ -z "$out" ]]; then
  safe="${branch//\//-}"
  out="/tmp/${safe}-verification-urls.md"
fi

# Ahead/behind counts.
ahead="?"
behind="?"
if [[ "$ref" == "HEAD" ]]; then
  if git rev-parse --abbrev-ref --symbolic-full-name '@{u}' >/dev/null 2>&1; then
    read -r ahead behind < <(git rev-list --left-right --count "HEAD...@{u}" | awk '{print $1" "$2}')
  fi
else
  if git show-ref --verify --quiet "refs/heads/$branch"; then
    read -r ahead behind < <(git rev-list --left-right --count "$branch...origin/$branch" | awk '{print $1" "$2}')
  fi
fi

if [[ "$urls_only" == "true" ]]; then
  for f in "${files[@]}"; do
    echo "https://raw.githubusercontent.com/$repo_slug/$branch/$f"
  done > "$out"
  echo "$out"
  exit 0
fi

local_exists="no"
if git show-ref --verify --quiet "refs/heads/$branch"; then
  local_exists="yes"
fi

{
  echo "# ${branch} - Raw GitHub URLs for Verification"
  echo
  echo "Branch: \`$branch\`"
  echo "Repo: \`https://github.com/$repo_slug\`"
  echo
  echo "## Modified / Added Files"
  echo

  echo "### Cargo & Config"
  for f in "${files[@]}"; do
    [[ "$f" == "Cargo.toml" || "$f" == "Cargo.lock" || "$f" == *.toml || "$f" == *.yml || "$f" == *.yaml ]] || continue
    echo "- $f: https://raw.githubusercontent.com/$repo_slug/$branch/$f"
  done
  echo
  echo "### Source Files"
  for f in "${files[@]}"; do
    [[ "$f" == src/* ]] || continue
    echo "- $f: https://raw.githubusercontent.com/$repo_slug/$branch/$f"
  done
  echo
  echo "### Test Files"
  for f in "${files[@]}"; do
    [[ "$f" == tests/* || "$f" == test/* ]] || continue
    echo "- $f: https://raw.githubusercontent.com/$repo_slug/$branch/$f"
  done
  echo
  echo "### Other"
  for f in "${files[@]}"; do
    [[ "$f" == src/* || "$f" == tests/* || "$f" == test/* \
       || "$f" == Cargo.toml || "$f" == Cargo.lock \
       || "$f" == *.toml || "$f" == *.yml || "$f" == *.yaml ]] && continue
    echo "- $f: https://raw.githubusercontent.com/$repo_slug/$branch/$f"
  done

  echo
  echo "---"
  echo "Generated: $(date)"
  echo "* Branch \`$branch\` exists on origin. Local branch exists: \`$local_exists\`."
  echo "* Ref used for diff/sha: \`$ref\`."
  echo "* Ref commit: $short_sha ($head_sha)."
  echo "* Local vs upstream: ahead/behind is ${ahead}/${behind}."
  echo "* Changed files (${#files[@]}):"
  for f in "${files[@]}"; do
    echo "  * $f"
  done
  echo "* Working tree is clean."
} > "$out"

echo "$out"
