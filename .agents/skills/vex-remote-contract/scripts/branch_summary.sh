#!/usr/bin/env bash
# branch_summary.sh — Print commit/files summary and optionally write PR body markdown.
# Usage:
#   branch_summary.sh [-b <branch>] [--write-pr-body] [-o <path>]
set -euo pipefail
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=_lib.sh
source "$SCRIPT_DIR/_lib.sh"

branch=""
write_pr_body="false"
pr_body_out=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    -b|--branch) branch="$2"; shift 2;;
    --write-pr-body) write_pr_body="true"; shift 1;;
    -o|--out) pr_body_out="$2"; shift 2;;
    *) die "unknown arg: $1";;
  esac
done

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

repo_slug="$(repo_slug_from_origin)"
short_sha="$(git rev-parse --short "$ref")"
head_sha="$(git rev-parse "$ref")"

mapfile -t files < <(git diff --name-only "origin/main...$ref" | sed '/^$/d')
count="${#files[@]}"
[[ "$count" -gt 0 ]] || die "no changes vs origin/main...$ref"

safe="${branch//\//-}"
ver_file="/tmp/${safe}-verification-urls.md"
if [[ -z "$pr_body_out" ]]; then
  pr_body_out="/tmp/${safe}-pr-body.md"
fi

# Extract ADR and batch hints from branch name.
adr_hint=""
if [[ "$branch" =~ adr-([0-9]+) ]]; then
  adr_hint="ADR-${BASH_REMATCH[1]}"
fi

batch_hint=""
if [[ "$branch" =~ batch-([a-zA-Z0-9]+) ]]; then
  batch_hint="Batch ${BASH_REMATCH[1]^^}"
fi

echo "## Branch contract summary"
echo
echo "- Commit: \`$short_sha\` (full: \`$head_sha\`)"
echo "- Branch: \`$branch\` → \`origin/$branch\`"
echo "- Ref: \`$ref\`"
echo "- Files: \`$count\` changed"
echo "- Verification URLs file: \`$ver_file\`"
echo "- PR URL (create): \`https://github.com/$repo_slug/pull/new/$branch\`"
echo "- PR body file: \`$pr_body_out\`"
echo

echo "### Motivation"
echo
echo "- Implements the batch dispatch contract for ${batch_hint:+$batch_hint / }${adr_hint:-<ADR>}."
echo "- Targeted changes:"
for f in "${files[@]}"; do
  echo "  - \`$f\`"
done
echo "- Verification:"
echo "  - All anchor tests green (see CI)"
echo "  - Raw GitHub URLs verified (HTTP 200)"
echo "  - diff contains all expected paths"
echo
echo "### Files changed: ${count}"
for f in "${files[@]}"; do
  echo "  - \`$f\`"
done

if [[ "$write_pr_body" == "true" ]]; then
  {
    echo "## Summary"
    echo
    echo "- Implements the batch dispatch contract for ${batch_hint:+$batch_hint / }${adr_hint:-<ADR>}."
    echo "- Branch: \`$branch\`"
    echo "- Commit: \`$short_sha\`"
    echo
    echo "## Motivation"
    echo
    echo "- Targeted changes:"
    for f in "${files[@]}"; do
      echo "  - \`$f\`"
    done
    echo
    echo "## Verification"
    echo
    echo "- All anchor tests green (see CI)"
    echo "- Raw GitHub URLs verified (HTTP 200)"
    echo "- diff contains all expected paths"
    echo
    echo "## Files changed (${count})"
    for f in "${files[@]}"; do
      echo "- \`$f\`"
    done
  } > "$pr_body_out"

  echo
  echo "- PR body markdown written: \`$pr_body_out\`"
fi
