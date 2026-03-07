#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
PUBLISH_BRANCH="${PUBLISH_BRANCH:-gh-pages}"
WORKTREE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vexcoder-${PUBLISH_BRANCH}-XXXXXX")"

cleanup() {
  git -C "$ROOT" worktree remove --force "$WORKTREE_DIR" >/dev/null 2>&1 || true
}
trap cleanup EXIT

command -v mdbook >/dev/null 2>&1 || {
  echo "MISSING TOOL: mdbook"
  echo "  Install: cargo install mdbook --version 0.5.0 --locked"
  exit 1
}

cd "$ROOT"
bash scripts/generate-docs.sh
mdbook build docs

git fetch origin "$PUBLISH_BRANCH" >/dev/null 2>&1 || true

if git show-ref --verify --quiet "refs/remotes/origin/${PUBLISH_BRANCH}"; then
  git worktree add --detach "$WORKTREE_DIR" "origin/${PUBLISH_BRANCH}"
  git -C "$WORKTREE_DIR" switch -C "$PUBLISH_BRANCH"
else
  git worktree add --detach "$WORKTREE_DIR"
  git -C "$WORKTREE_DIR" checkout --orphan "$PUBLISH_BRANCH"
fi

find "$WORKTREE_DIR" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
cp -R "$ROOT/docs/book/." "$WORKTREE_DIR/"
touch "$WORKTREE_DIR/.nojekyll"

git -C "$WORKTREE_DIR" add -A

printf '%s\n' \
  "Prepared local publish worktree:" \
  "  $WORKTREE_DIR" \
  "" \
  "Next steps:" \
  "  cd \"$WORKTREE_DIR\"" \
  "  git status --short" \
  "  git commit -m \"docs: publish site\"" \
  "  git push origin HEAD:${PUBLISH_BRANCH}"
