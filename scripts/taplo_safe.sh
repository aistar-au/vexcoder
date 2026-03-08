#!/usr/bin/env bash
# taplo_safe.sh — run taplo checks, but tolerate the known Darwin
# system-configuration panic by falling back to TOML syntax validation only.
#
# This keeps CI behavior unchanged on Linux while allowing local macOS gates to
# proceed when taplo crashes before it can inspect any repository files.
set -euo pipefail

MODE="${1:-}"
case "$MODE" in
  fmt-check) shift; CMD=(taplo fmt --check --diff "$@");;
  lint) shift; CMD=(taplo lint "$@");;
  *) echo "taplo_safe.sh: ERROR: expected fmt-check or lint" >&2; exit 1;;
esac

REPO_ROOT="$(git rev-parse --show-toplevel)"
STDOUT_FILE="$(mktemp)"
STDERR_FILE="$(mktemp)"
trap 'rm -f "$STDOUT_FILE" "$STDERR_FILE"' EXIT

if "${CMD[@]}" >"$STDOUT_FILE" 2>"$STDERR_FILE"; then
  cat "$STDOUT_FILE"
  cat "$STDERR_FILE" >&2
  exit 0
else
  STATUS=$?
  STDERR_TEXT="$(cat "$STDERR_FILE")"
fi

is_known_darwin_taplo_panic() {
  [[ "$(uname -s)" == "Darwin" ]] &&
  [[ "$STATUS" -eq 101 ]] &&
  [[ "$STDERR_TEXT" == *"system-configuration"* ]] &&
  [[ "$STDERR_TEXT" == *"Attempted to create a NULL object."* ]]
}

if is_known_darwin_taplo_panic; then
  python3 - "$REPO_ROOT" <<'PY'
import subprocess
import sys
import tomllib
from pathlib import Path

root = Path(sys.argv[1])
result = subprocess.run(
    ["git", "-C", str(root), "ls-files", "*.toml"],
    check=True,
    capture_output=True,
    text=True,
)

failures = []
for rel in [line for line in result.stdout.splitlines() if line.strip()]:
    path = root / rel
    try:
        tomllib.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:  # pragma: no cover - shell fallback path
        failures.append(f"{rel}: {exc}")

if failures:
    print("taplo_safe.sh: TOML syntax fallback failed:", file=sys.stderr)
    for failure in failures:
        print(f"- {failure}", file=sys.stderr)
    sys.exit(1)
PY
  echo "taplo_safe.sh: WARN: taplo crashed with the known Darwin system-configuration panic; falling back to TOML syntax validation only for $MODE." >&2
  exit 0
fi

cat "$STDOUT_FILE"
printf '%s' "$STDERR_TEXT" >&2
exit "$STATUS"
