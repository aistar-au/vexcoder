#!/usr/bin/env bash
set -euo pipefail

PYTHON_BIN=()
if command -v py >/dev/null 2>&1; then
  PYTHON_BIN=(py -3)
elif command -v python3 >/dev/null 2>&1; then
  PYTHON_BIN=(python3)
elif command -v python >/dev/null 2>&1; then
  PYTHON_BIN=(python)
fi

RG_BIN="${VEX_RG_BIN:-rg}"
SCAN_BACKEND="rg"
if [[ "$RG_BIN" == *"/"* || "$RG_BIN" == *"\\"* ]]; then
  if [[ ! -x "$RG_BIN" ]]; then
    echo "FAIL: ripgrep executable not found at $RG_BIN" >&2
    exit 1
  fi
elif ! command -v "$RG_BIN" >/dev/null 2>&1; then
  if command -v rg.exe >/dev/null 2>&1; then
    RG_BIN="rg.exe"
  elif [[ ${#PYTHON_BIN[@]} -gt 0 ]]; then
    SCAN_BACKEND="python"
  else
    echo "FAIL: ripgrep executable not found (expected rg, rg.exe, or VEX_RG_BIN), and no python fallback is available" >&2
    exit 1
  fi
fi

scan_targets() {
  local pattern="$1"
  shift
  if [[ "$SCAN_BACKEND" == "rg" ]]; then
    "$RG_BIN" -n --hidden -i \
      --glob '!.git' \
      --glob '!.github/agents/vexcoder-ui-parity-orchestrator.agent.md' \
      --glob '!.github/agents/vexcoder-ui-paragraph-renderer.agent.md' \
      --glob '!.github/agents/vexcoder-transcript-renderer-overhaul.agent.md' \
      --glob '!.github/agents/vexcoder-hybrid-retrieval.agent.md' \
      --glob '!.github/workflows/**' \
      --glob '!scripts/check_forbidden_names.sh' \
      --glob '!TASKS/completed/REPO-RAW-URL-MAP.md' \
      "$pattern" "$@" | sed 's#\\#/#g'
    return
  fi

  "${PYTHON_BIN[@]}" - "$pattern" "$@" <<'PY'
from pathlib import Path
import re
import sys

pattern = re.compile(sys.argv[1], re.IGNORECASE)
roots = [Path(arg) for arg in sys.argv[2:]]
matched = False

for root in roots:
    if not root.exists():
        continue
    paths = [root] if root.is_file() else [p for p in root.rglob("*") if p.is_file()]
    for path in paths:
        rel = path.as_posix()
        if rel == "scripts/check_forbidden_names.sh":
            continue
        if rel == "TASKS/completed/REPO-RAW-URL-MAP.md":
            continue
        if rel == ".github/agents/vexcoder-ui-parity-orchestrator.agent.md":
            continue
        if rel == ".github/agents/vexcoder-ui-paragraph-renderer.agent.md":
            continue
        if rel == ".github/agents/vexcoder-transcript-renderer-overhaul.agent.md":
            continue
        if rel == ".github/agents/vexcoder-hybrid-retrieval.agent.md":
            continue
        if rel.startswith(".git/") or rel.startswith(".github/workflows/"):
            continue
        try:
            lines = path.read_text(encoding="utf-8", errors="ignore").splitlines()
        except OSError as exc:
            print(f"FAIL: unable to read {rel}: {exc}", file=sys.stderr)
            sys.exit(2)
        for line_number, line in enumerate(lines, start=1):
            if pattern.search(line):
                print(f"{rel}:{line_number}:{line}")
                matched = True

sys.exit(0 if matched else 1)
PY
}

scan_workflows() {
  if [[ "$SCAN_BACKEND" == "rg" ]]; then
    "$RG_BIN" -n --hidden -i --glob '!.git' --glob '!.github/workflows/copilot-setup-steps.yml' "$BRAND_PATTERN" .github/workflows/ | sed 's#\\#/#g'
    return
  fi

  "${PYTHON_BIN[@]}" - "$BRAND_PATTERN" <<'PY'
from pathlib import Path
import re
import sys

pattern = re.compile(sys.argv[1], re.IGNORECASE)
root = Path(".github/workflows")
matched = False

for path in root.rglob("*"):
    if not path.is_file():
        continue
    rel = path.as_posix()
    if rel == ".github/workflows/copilot-setup-steps.yml":
        continue
    try:
        lines = path.read_text(encoding="utf-8", errors="ignore").splitlines()
    except OSError as exc:
        print(f"FAIL: unable to read {rel}: {exc}", file=sys.stderr)
        sys.exit(2)
    for line_number, line in enumerate(lines, start=1):
        if pattern.search(line):
            print(f"{rel}:{line_number}:{line}")
            matched = True

sys.exit(0 if matched else 1)
PY
}

scan_path_names() {
  local pattern="$1"
  shift
  if [[ ${#PYTHON_BIN[@]} -gt 0 ]]; then
    "${PYTHON_BIN[@]}" - "$pattern" "$@" <<'PY'
from pathlib import Path
import re
import sys

pattern = re.compile(sys.argv[1], re.IGNORECASE)
roots = [Path(arg) for arg in sys.argv[2:]]
matched = False

for root in roots:
    if not root.exists():
        continue
    paths = [root] if root.is_file() else [p for p in root.rglob("*") if p.is_file()]
    for path in paths:
        rel = path.as_posix()
        if pattern.search(rel):
            print(rel)
            matched = True

sys.exit(0 if matched else 1)
PY
    return
  fi

  local path_root
  local matched=1
  for path_root in "$@"; do
    if [[ ! -d "$path_root" ]]; then
      continue
    fi
    if find "$path_root" -type f -print | "$RG_BIN" -n -i "$pattern" | sed 's#\\#/#g'; then
      matched=0
    fi
  done
  return "$matched"
}

# Keep this check scoped to proprietary/vendor-branded terms and
# external repository-backed identifiers that are disallowed in
# agent/workflow surfaces.
# The caret/editor collision term is intentionally not matched as a
# standalone token because it collides with legitimate variable names
# across the codebase.
#
# Two-pass design:
#   Pass 1 (PATTERN): full pattern across all targets, *excluding* .github/workflows/**
#           Workflow files legitimately reference pinned third-party actions
#           (actions/checkout, dtolnay/rust-toolchain, etc.) via `uses:` directives.
#           Those are not disallowed in CI; they are disallowed in agent/skill surfaces.
#           The generated raw URL map is also excluded because it must mechanically
#           mirror tracked repository paths, including platform-mandated filenames.
#   Pass 2 (BRAND_PATTERN): brand-name-only subset, *including* .github/workflows/**
#           Ensures no proprietary assistant-brand names appear in workflow YAML even
#           though action-reference patterns are excluded there.
#           Exact-path allowlists cover the repository's required coding-agent
#           setup workflow and its paired repository-level agent profiles
#           because those files must use platform-mandated integration
#           identifiers.
#   Pass 3 (PATH_PATTERN): brand-name subset over prompt/model file paths so
#           branded fixture/template filenames are rejected even when file
#           contents are generic.
brand_words=(
  $'\x63\x6c\x61\x75\x64\x65'
  $'\x61\x6e\x74\x68\x72\x6f\x70\x69\x63'
  $'\x67\x6f\x6f\x67\x6c\x65'
  $'\x63\x6f\x64\x65\x78'
  $'\x6f\x70\x65\x6e\x61\x69'
  $'\x67\x70\x74'
  $'\x63\x6f\x70\x69\x6c\x6f\x74'
  $'\x67\x65\x6d\x69\x6e\x69'
  $'\x71\x77\x65\x6e'
  $'\x64\x65\x65\x70\x73\x65\x65\x6b'
  $'\x63\x6f\x64\x65\x6c\x6c\x61\x6d\x61'
  $'\x73\x74\x61\x72\x63\x6f\x64\x65\x72'
  $'\x63\x6f\x64\x65\x77\x68\x69\x73\x70\x65\x72\x65\x72'
)
brand_regex="$(printf '%s|' "${brand_words[@]}")"
brand_regex="${brand_regex%|}"
caret_host=$'\x63\x75\x72\x73\x6f\x72\\.com'
caret_phrase=$'\\b\x63\x75\x72\x73\x6f\x72 ai\\b'
editor_brand=$'\\bVS Code\\b'

PATTERN="\\b(${brand_regex})\\b|${caret_host}|${caret_phrase}|peter-evans/create-pull-request|leonardomso/rust-skills|actions/checkout|actions/cache|actions/upload-pages-artifact|actions/deploy-pages|dtolnay/rust-toolchain|uncenter/setup-taplo|\\bvexcoder/vexcoder\\b|${editor_brand}"

BRAND_PATTERN="\\b(${brand_regex})\\b|${caret_host}|${caret_phrase}|${editor_brand}"

TARGETS=(
  .gitignore
  AGENTS.md
  CONTRIBUTING.md
  TASKS
  adr
  docs/src
  src
  tests
  scripts
  .github
  Makefile
)
if [[ -d models ]]; then
  TARGETS+=(models)
fi

failed=0

# Pass 1: full pattern — .github/workflows/** excluded (.github non-workflow files still scanned)
if scan_targets "$PATTERN" "${TARGETS[@]}"; then
  failed=1
fi

# Pass 2: brand names only — also covers .github/workflows/ (no AI brand names in CI YAML)
if [[ -d .github/workflows ]] && \
   scan_workflows; then
  failed=1
fi

# Pass 3: filenames in src/prompts/ and models/ must also stay generic.
if scan_path_names "$BRAND_PATTERN" src/prompts models; then
  failed=1
fi

if [[ $failed -ne 0 ]]; then
  echo "FAIL: forbidden branded names found in ${TARGETS[*]}"
  exit 1
fi

echo "clean"
