#!/usr/bin/env bash
# _lib.sh — shared helpers for vex-remote-contract scripts
# Source this file at the top of each script:
#   SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
#   source "$SCRIPT_DIR/_lib.sh"

die() {
  echo "ERROR: $*" >&2
  exit 1
}

# Extract "owner/repo" from the origin remote URL.
# Handles both https://github.com/owner/repo.git and git@github.com:owner/repo.git forms.
repo_slug_from_origin() {
  local url
  url="$(git remote get-url origin 2>/dev/null)" || die "no remote named 'origin'"
  # Strip trailing .git, then extract the owner/repo portion.
  url="${url%.git}"
  # ssh form: git@github.com:owner/repo
  if [[ "$url" =~ github\.com[:/](.+/[^/]+)$ ]]; then
    echo "${BASH_REMATCH[1]}"
  else
    die "cannot parse repo slug from origin URL: $url"
  fi
}

# Portable SHA-256 of a file — works on Linux (sha256sum) and macOS (shasum -a 256).
sha256_file() {
  local f="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$f" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$f" | awk '{print $1}'
  else
    die "neither sha256sum nor shasum found on PATH"
  fi
}
