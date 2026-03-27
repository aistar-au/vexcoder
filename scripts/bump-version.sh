#!/usr/bin/env bash
# bump-version.sh — update the version string across the repository.
#
# Usage:
#   bash scripts/bump-version.sh <new-version>
#
# Example:
#   bash scripts/bump-version.sh 0.1.0-alpha.4
#
# The script:
#   1. Validates the version against semver format.
#   2. Reads the current version from Cargo.toml.
#   3. Updates Cargo.toml, Cargo.lock, CONTRIBUTING.md, and RELEASING.md.
#   4. Prints a summary of changed files.
#
# The version argument must NOT include the "v" prefix.

set -euo pipefail

NEW_VERSION="${1:-}"

if [ -z "$NEW_VERSION" ]; then
  echo "Usage: bash scripts/bump-version.sh <new-version>"
  echo "Example: bash scripts/bump-version.sh 0.1.0-alpha.4"
  exit 1
fi

# Strip leading "v" if accidentally provided.
NEW_VERSION="${NEW_VERSION#v}"

# Validate semver format: MAJOR.MINOR.PATCH[-PRERELEASE]
if ! echo "$NEW_VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9]+(\.[a-zA-Z0-9]+)*)?$'; then
  echo "FAIL: '$NEW_VERSION' is not a valid semver string."
  echo "Expected format: MAJOR.MINOR.PATCH or MAJOR.MINOR.PATCH-PRERELEASE"
  echo "Examples: 0.1.0, 0.1.0-alpha.3, 1.0.0-beta.1, 1.0.0-rc.1"
  exit 1
fi

# Read current version from Cargo.toml.
CURRENT_VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/version = "\(.*\)"/\1/')

if [ -z "$CURRENT_VERSION" ]; then
  echo "FAIL: could not read current version from Cargo.toml"
  exit 1
fi

if [ "$CURRENT_VERSION" = "$NEW_VERSION" ]; then
  echo "Version is already $NEW_VERSION — nothing to do."
  exit 0
fi

echo "Bumping version: $CURRENT_VERSION -> $NEW_VERSION"

# Escape dots and hyphens for sed patterns.
ESC_OLD=$(echo "$CURRENT_VERSION" | sed 's/[.]/\\./g')
ESC_NEW="$NEW_VERSION"

# Portable in-place sed: macOS requires -i '', GNU requires -i (no arg).
sedi() {
  if sed --version 2>/dev/null | grep -q GNU; then
    sed -i "$@"
  else
    sed -i '' "$@"
  fi
}

# 1. Cargo.toml — update the [package] version line.
sedi "s/^version = \"$ESC_OLD\"/version = \"$ESC_NEW\"/" Cargo.toml
echo "  Updated Cargo.toml"

# 2. Cargo.lock — run cargo check to regenerate.
cargo check --quiet 2>/dev/null
echo "  Updated Cargo.lock (via cargo check)"

# 3. CONTRIBUTING.md — version header and packaging examples.
if [ -f CONTRIBUTING.md ]; then
  sedi "s/$ESC_OLD/$ESC_NEW/g" CONTRIBUTING.md
  echo "  Updated CONTRIBUTING.md"
fi

# 4. RELEASING.md — example version references.
if [ -f RELEASING.md ]; then
  sedi "s/$ESC_OLD/$ESC_NEW/g" RELEASING.md
  echo "  Updated RELEASING.md"
fi

echo ""
echo "Version bumped to $NEW_VERSION."
echo ""
echo "Next steps:"
echo "  1. Review the changes: git diff"
echo "  2. Commit: git add Cargo.toml Cargo.lock CONTRIBUTING.md RELEASING.md"
echo "  3. After merge, tag: git tag -a v$NEW_VERSION -m \"Release v$NEW_VERSION\""
echo "  4. Push the tag: git push origin v$NEW_VERSION"
echo "  5. The tag-triggered release workflow publishes the release notes and changelog asset automatically"
