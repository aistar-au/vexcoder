# ==============================================================================
# vexcoder — Makefile v0
#
# Wraps the exact gates that already run across ci.yml and arch-contracts.yml.
# No new checks. No new scripts. No phantom targets.
#
# Usage:
#   make gate          full gate (matches ci.yml + arch-contracts.yml combined)
#   make gate-fast     gate without map-check (tight local edit loop)
#   make fix           apply fmt + taplo + line-ending renorm + map-update
#   make help          list all targets
#
# CI migration (after this file lands on main):
#   ci.yml            replace individual steps with: make gate
#   arch-contracts    replace individual steps with: make check-arch && make test-targets
#   autofix.yml       replace fmt/taplo steps with:  make fix
#                     keep: checkout, toolchain install, peter-evans PR creation
#                     (those steps need GitHub token permissions — not a logic concern)
#
# Tool prerequisites:
#   taplo   cargo install taplo-cli --version 0.8.1  (CI pins via setup-taplo@v1)
#   rg      cargo install ripgrep  OR  apt install ripgrep  (check_forbidden_names.sh only)
#   rust    rustup (stable toolchain)
#
# Future targets (not included — scripts do not yet exist on main):
#   release        scripts/release.sh              (ADR-024 Gap 9 — Phase G)
#   health-check   scripts/agent_health_check.sh   (ADR-024 Gap 27)
#   lint-json      scripts/lint_check.sh           (structured clippy for agents)
# ==============================================================================

SHELL := bash
.SHELLFLAGS := -euo pipefail -c

.PHONY: help \
        _require-taplo _require-rg \
        build check \
        fmt fmt-check \
        lint \
        check-boundary check-routing check-imports check-names check-module-names check-arch \
        map-check map-check-full map-update \
        test test-targets test-single \
        gate gate-fast \
        fix \
        clean


# ------------------------------------------------------------------------------
# Help — explicit list, no regex
# ------------------------------------------------------------------------------
help:
	@printf '%s\n' \
	  "Targets:" \
	  "  help               show this help" \
	  "  build              cargo build --all-targets" \
	  "  check              cargo check --all-targets" \
	  "  fmt                cargo fmt + taplo fmt (write)" \
	  "  fmt-check          cargo fmt --check + taplo fmt --check + taplo lint" \
	  "  lint               cargo clippy --all-targets -- -D warnings" \
	  "  check-boundary     assert no ratatui/crossterm in src/runtime/ (ADR-006)" \
	  "  check-routing      assert no alternate routing patterns (ADR-007, ADR-014)" \
	  "  check-imports      assert no forbidden cross-layer imports (ADR-007)" \
	  "  check-names        assert no proprietary vendor brand names (ADR-023)" \
	  "  check-module-names assert Rust 2018 path-based modules — no mod.rs files" \
	  "  check-arch         all architecture boundary checks (ci.yml + arch-contracts.yml)" \
	  "  map-check          index-only map drift check — fails only on missing tracked files" \
	  "  map-check-full     full byte-for-byte map sync check including line counts" \
	  "  map-update         regenerate REPO-RAW-URL-MAP (full sync)" \
	  "  test               cargo test --all with VEX_MODEL_TOKEN=\"\" (ci.yml variant)" \
	  "  test-targets       cargo test --all-targets (arch-contracts.yml variant)" \
	  "  test-single        run one test by name: make test-single T=test_fn_name" \
	  "  gate               FULL gate: ci.yml + arch-contracts.yml + map index check" \
	  "  gate-fast          fast gate: full gate minus map-check (local edit loop)" \
	  "  fix                rustfmt + taplo + renorm + map-update (all auto-fixable in one pass)" \
	  "  clean              cargo clean"


# ------------------------------------------------------------------------------
# Tool guards — fail hard with a clear install message.
#
# _require-taplo  prereq for: fmt-check, fmt, fix
# _require-rg     prereq for: check-names ONLY
#
# Why not check-routing or check-imports:
#   check_no_alternate_routing.sh  uses grep -rn  (POSIX, always available)
#   check_forbidden_imports.sh     uses grep -rn  (POSIX, always available)
#   check_forbidden_names.sh       uses rg        (ripgrep — guard required)
# ------------------------------------------------------------------------------
_require-taplo:
	@command -v taplo >/dev/null 2>&1 || { \
	  echo ""; \
	  echo "MISSING TOOL: taplo"; \
	  echo "  Install: cargo install taplo-cli --version 0.8.1"; \
	  echo "  (CI pins this version via uncenter/setup-taplo@v1)"; \
	  echo ""; \
	  exit 1; \
	}

_require-rg:
	@command -v rg >/dev/null 2>&1 || { \
	  echo ""; \
	  echo "MISSING TOOL: rg (ripgrep)"; \
	  echo "  Required by: check_forbidden_names.sh"; \
	  echo "  Install (cargo):  cargo install ripgrep"; \
	  echo "  Install (apt):    sudo apt-get install ripgrep"; \
	  echo "  Install (brew):   brew install ripgrep"; \
	  echo ""; \
	  exit 1; \
	}


# ------------------------------------------------------------------------------
# Build
# ------------------------------------------------------------------------------
build:
	cargo build --all-targets

check:
	cargo check --all-targets


# ------------------------------------------------------------------------------
# Format
# Source: ci.yml steps "Format (rustfmt)", "TOML format (taplo)", "TOML lint"
# ------------------------------------------------------------------------------
fmt: _require-taplo
	cargo fmt
	taplo fmt

fmt-check: _require-taplo
	cargo fmt --check
	taplo fmt --check --diff
	taplo lint


# ------------------------------------------------------------------------------
# Lint
# Source: ci.yml step "Clippy (deny warnings)"
# ------------------------------------------------------------------------------
lint:
	cargo clippy --all-targets -- -D warnings


# ------------------------------------------------------------------------------
# Architecture boundary checks
#
# Source mapping:
#   check-boundary      inline grep — ci.yml "No UI crates in src/runtime"
#   check-module-names  inline find — ci.yml "Enforce Rust 2018 module entry naming"
#   check-names         scripts/check_forbidden_names.sh — ci.yml (uses rg)
#   check-routing       scripts/check_no_alternate_routing.sh — arch-contracts.yml
#   check-imports       scripts/check_forbidden_imports.sh — arch-contracts.yml
# ------------------------------------------------------------------------------
check-boundary:
	@if grep -r -F "ratatui" src/runtime/ || grep -r -F "crossterm" src/runtime/; then \
	  echo ""; \
	  echo "FAIL check-boundary: terminal crates found in src/runtime/"; \
	  echo "  See: ADR-006 (runtime mode contracts)"; \
	  exit 1; \
	fi
	@echo "check-boundary: clean"

check-routing:
	@bash scripts/check_no_alternate_routing.sh

check-imports:
	@bash scripts/check_forbidden_imports.sh

check-names: _require-rg
	@./scripts/check_forbidden_names.sh

check-module-names:
	@first="$$(find src -mindepth 2 -maxdepth 2 -type f -name 'mod.rs' | head -n 1)"; \
	if [[ -n "$$first" ]]; then \
	  echo ""; \
	  echo "FAIL check-module-names: mod.rs found — Rust 2018 path-based modules required"; \
	  find src -mindepth 2 -maxdepth 2 -type f -name 'mod.rs' | sort; \
	  exit 1; \
	fi
	@echo "check-module-names: clean"

check-arch: \
  check-boundary \
  check-routing \
  check-imports \
  check-names \
  check-module-names
	@echo "check-arch: all boundaries clean"


# ------------------------------------------------------------------------------
# Repo map gate
# Source: onboarding Hard Rule 8, vex-remote-contract Step 9 MAP GATE
#
# Policy (decided 2026-03-06): INDEX-ONLY mode via --check-index.
#
#   map-check        --check-index: fails only when tracked files are absent
#                    from the index. Normal edits changing line counts do NOT
#                    fail this gate. Used by make gate and CI.
#
#   map-check-full   --check: byte-for-byte sync including line counts and
#                    ordering. Use before releases or explicit full-sync PRs.
#
#   map-update       regenerates the full map (called automatically by fix).
#
# Requires: update_repo_raw_url_map.sh --check-index flag added via patch
#   0001-add-check-index-flag.patch (apply before landing this Makefile)
# ------------------------------------------------------------------------------
map-check:
	@.agents/skills/vex-remote-contract/scripts/update_repo_raw_url_map.sh --check-index

map-check-full:
	@.agents/skills/vex-remote-contract/scripts/update_repo_raw_url_map.sh --check

map-update:
	@.agents/skills/vex-remote-contract/scripts/update_repo_raw_url_map.sh


# ------------------------------------------------------------------------------
# Tests
#
# Two variants preserved — policy: keep both (decided 2026-03-06).
#
# test         cargo test --all    with VEX_MODEL_TOKEN=""
#              Source: ci.yml — env guard prevents accidental real API calls
#
# test-targets cargo test --all-targets  (no token env override)
#              Source: arch-contracts.yml — broader target set
#
# Both run in make gate. Removing either changes CI coverage semantics.
# ------------------------------------------------------------------------------
test:
	VEX_MODEL_TOKEN="" cargo test --all

test-targets:
	cargo test --all-targets

test-single:
	cargo test $(T) --all-targets


# ------------------------------------------------------------------------------
# Full gate
#
# gate       = ci.yml + arch-contracts.yml + map index check (in order)
# gate-fast  = same minus map-check (for tight local edit loops)
#
# Execution order follows ci.yml, with arch-contracts checks appended.
# map-check runs last: pure shell (git ls-files + awk), only fails on
# file-set changes (index-only policy), zero cost on normal edits.
# ------------------------------------------------------------------------------
gate: \
  fmt-check \
  lint \
  check \
  check-arch \
  test \
  test-targets \
  map-check
	@echo ""
	@echo "gate: all checks passed"

gate-fast: \
  fmt-check \
  lint \
  check \
  check-arch \
  test \
  test-targets
	@echo ""
	@echo "gate-fast: passed (map-check skipped — run 'make gate' before push)"


# ------------------------------------------------------------------------------
# Autofix
#
# Covers: rustfmt, taplo fmt, git line-ending renormalization, map regeneration.
# map-update is included so the index is current before you commit.
# Commit all staged changes together (fmt + map) after running fix.
#
# Does NOT cover: checkout, toolchain install, peter-evans PR creation.
# Those require GitHub token permissions and stay in YAML.
# ------------------------------------------------------------------------------
fix: _require-taplo
	cargo fmt
	taplo fmt
	git add --renormalize .
	@.agents/skills/vex-remote-contract/scripts/update_repo_raw_url_map.sh
	git add TASKS/completed/REPO-RAW-URL-MAP.md
	@echo ""
	@echo "fix: applied — run 'make gate' to verify"


# ------------------------------------------------------------------------------
# Clean
# ------------------------------------------------------------------------------
clean:
	cargo clean
