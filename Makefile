# ==============================================================================
# vexcoder — Makefile v0
#
# Wraps the exact gates that already run across ci.yml and arch-contracts.yml,
# plus the release packaging entrypoint used by release.yml.
# No new checks. No phantom targets.
#
# Usage:
#   make gate          full gate (matches ci.yml + arch-contracts.yml combined)
#   make gate-fast     gate without map-check (tight local edit loop)
#   make release       package one target to dist/ for local smoke testing
#   make fix           apply fmt + taplo + line-ending renorm
#   make help          list all targets
#
# Tool prerequisites:
#   taplo   cargo install taplo-cli --version 0.8.1  (CI pins this version in workflow)
#   rg      cargo install ripgrep  OR  apt install ripgrep  (check_forbidden_names.sh only)
#   rust    rustup (stable toolchain)
# ==============================================================================

SHELL := bash
.SHELLFLAGS := -euo pipefail -c

.PHONY: help \
        _require-taplo _require-rg \
        build check \
        fmt fmt-check \
        lint \
        commit-debug-gate \
        check-boundary check-routing check-imports check-names check-module-names check-arch \
        test test-targets test-single \
        release \
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
	  "  commit-debug-gate  self-contained pre-push check for src/tests edits (runs gate-fast)" \
	  "  check-boundary     assert no ratatui/crossterm in src/runtime/ (ADR-006)" \
	  "  check-routing      assert no alternate routing patterns (ADR-007, ADR-014)" \
	  "  check-imports      assert no forbidden cross-layer imports (ADR-007)" \
	  "  check-names        assert no proprietary vendor brand names (ADR-023)" \
	  "  check-module-names assert Rust 2018 path-based modules — no mod.rs files" \
	  "  check-arch         all architecture boundary checks (ci.yml + arch-contracts.yml)" \
	  "  test               cargo test --all with VEX_MODEL_TOKEN=\"\" (ci.yml variant)" \
	  "  test-targets       cargo test --all-targets (arch-contracts.yml variant)" \
	  "  test-single        run one test by name: make test-single T=test_fn_name" \
	  "  gate               full gate: ci.yml + arch-contracts.yml" \
	  "  gate-fast          alias for gate (map-check removed)" \
	  "  release            package one target: make release VERSION=v0.1.0-alpha.1 TARGET=x86_64-unknown-linux-gnu" \
	  "  fix                rustfmt + taplo + renorm (all auto-fixable in one pass)" \
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
	  echo "  (CI pins this version in workflow)"; \
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
# Commit-debug gate
#
# Required before push when changed paths include `src/**/*.rs` or `tests/**/*.rs`.
# This repo must stay self-contained: no sibling repo or external devops checkout
# is required to validate local packaging or code changes. Reuse the existing
# local fast gate rather than shelling out to ../vexdraft.
# ------------------------------------------------------------------------------
commit-debug-gate: gate-fast
	@echo "commit-debug-gate: passed (self-contained local verification)"


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
# gate / gate-fast  = ci.yml + arch-contracts.yml (identical — map-check removed)
# ------------------------------------------------------------------------------
gate: \
  fmt-check \
  lint \
  check \
  check-arch \
  test \
  test-targets
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
	@echo "gate-fast: passed"


# ------------------------------------------------------------------------------
# Autofix
#
# Covers: rustfmt, taplo fmt, git line-ending renormalization.
# Does NOT cover: repository checkout, toolchain install, remote publish steps.
# Those require GitHub token permissions and stay in YAML.
# ------------------------------------------------------------------------------
fix: _require-taplo
	cargo fmt
	taplo fmt
	git add --renormalize .
	@echo ""
	@echo "fix: applied — run 'make gate' to verify"


# ------------------------------------------------------------------------------
# Release packaging
# ------------------------------------------------------------------------------
release:
	@VERSION="$(VERSION)" \
	 TARGET="$(TARGET)" \
	 OUT_DIR="$(if $(OUT_DIR),$(OUT_DIR),dist)" \
	 BUILD_TOOL="$(BUILD_TOOL)" \
	 bash scripts/release.sh


# ------------------------------------------------------------------------------
# Clean
# ------------------------------------------------------------------------------
clean:
	cargo clean
