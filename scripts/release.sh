#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  TARGET=x86_64-unknown-linux-gnu make release
  TARGET=x86_64-unknown-linux-gnu bash scripts/release.sh
  VERSION=v0.1.0-beta.6 TARGET=x86_64-unknown-linux-gnu bash scripts/release.sh
  VERSION=nightly TARGET=x86_64-unknown-linux-gnu bash scripts/release.sh

Inputs:
  VERSION / arg1   optional release tag; semver, short-SHA, or channel name
  TARGET  / arg2   Rust target triple to package
  OUT_DIR / arg3   output directory (default: dist)
  BUILD_TOOL       cargo or cross (default: cargo)
USAGE
}

normalize_version() {
  local version="$1"
  # Only prefix bare semver numbers with v; pass short-SHA and channel names through.
  if [[ "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
    version="v${version}"
  fi
  printf '%s\n' "${version}"
}

is_semver_tag() {
  [[ "$1" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]
}

is_short_sha_tag() {
  [[ "$1" =~ ^[0-9a-f]{7}$ ]]
}

is_channel_tag() {
  [[ "$1" == "nightly" ]]
}

run_with_retry() {
  local attempts="$1"
  local delay_seconds="$2"
  shift 2

  local attempt=1
  local exit_code=0
  while true; do
    if "$@"; then
      return 0
    fi
    exit_code=$?
    if (( attempt >= attempts )); then
      return "${exit_code}"
    fi
    echo "Build command failed with exit code ${exit_code}; retrying (${attempt}/${attempts}) in ${delay_seconds}s..." >&2
    sleep "${delay_seconds}"
    attempt=$((attempt + 1))
  done
}

derive_expected_version() {
  local pkgid
  local pkg_fragment
  pkgid="$(cargo pkgid --quiet 2>/dev/null)" || {
    echo "FAIL: could not read the Cargo package version via 'cargo pkgid --quiet'" >&2
    exit 1
  }
  pkg_fragment="${pkgid##*#}"
  printf 'v%s\n' "${pkg_fragment##*@}"
}

VERSION_INPUT="${VERSION:-${1:-}}"
TARGET="${TARGET:-${2:-}}"
OUT_DIR="${OUT_DIR:-${3:-dist}}"
BUILD_TOOL="${BUILD_TOOL:-cargo}"

if [[ -z "${TARGET}" ]]; then
  usage
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "FAIL: cargo is required" >&2
  exit 1
fi

EXPECTED_VERSION="$(derive_expected_version)"
if [[ -n "${VERSION_INPUT}" ]]; then
  VERSION="$(normalize_version "${VERSION_INPUT}")"
else
  VERSION="${EXPECTED_VERSION}"
fi

if is_semver_tag "${VERSION}"; then
  if [[ "${VERSION}" != "${EXPECTED_VERSION}" ]]; then
    echo "FAIL: semver VERSION ${VERSION} does not match Cargo package tag ${EXPECTED_VERSION}" >&2
    exit 1
  fi
elif is_short_sha_tag "${VERSION}" || is_channel_tag "${VERSION}"; then
  : # Non-semver tags do not require Cargo.toml alignment.
else
  echo "FAIL: VERSION must be a semver tag (v0.1.0), a 7-character short SHA, or a channel name (nightly/latest)" >&2
  exit 1
fi

case "${BUILD_TOOL}" in
  cargo|cross)
    ;;
  *)
    echo "FAIL: BUILD_TOOL must be 'cargo' or 'cross' (got '${BUILD_TOOL}')" >&2
    exit 1
    ;;
esac

if ! command -v "${BUILD_TOOL}" >/dev/null 2>&1; then
  echo "FAIL: ${BUILD_TOOL} is required" >&2
  exit 1
fi

ARCHIVE_VERSION="${VERSION#v}"
PACKAGE_DIR="vex-${ARCHIVE_VERSION}-${TARGET}"

case "${TARGET}" in
  *windows*)
    binary_name="vex.exe"
    archive_name="${PACKAGE_DIR}.zip"
    ;;
  *)
    binary_name="vex"
    archive_name="${PACKAGE_DIR}.tar.gz"
    ;;
esac

binary_path="target/${TARGET}/release/${binary_name}"
stage_dir="${OUT_DIR}/${PACKAGE_DIR}"
archive_path="${OUT_DIR}/${archive_name}"
checksum_path="${archive_path}.sha256"

mkdir -p "${OUT_DIR}"
rm -rf "${stage_dir}" "${archive_path}" "${checksum_path}"
mkdir -p "${stage_dir}"

build_cmd=("${BUILD_TOOL}" build --release --target "${TARGET}")
if [[ "${BUILD_TOOL}" == "cross" ]]; then
  CROSS_BUILD_RETRIES="${CROSS_BUILD_RETRIES:-3}"
  CROSS_BUILD_RETRY_DELAY_SECONDS="${CROSS_BUILD_RETRY_DELAY_SECONDS:-20}"
  run_with_retry "${CROSS_BUILD_RETRIES}" "${CROSS_BUILD_RETRY_DELAY_SECONDS}" "${build_cmd[@]}"
else
  "${build_cmd[@]}"
fi

if [[ ! -f "${binary_path}" ]]; then
  echo "FAIL: built binary not found at ${binary_path}" >&2
  exit 1
fi

install -m 755 "${binary_path}" "${stage_dir}/${binary_name}"
install -m 644 README.md "${stage_dir}/README.md"
install -m 644 LICENSE "${stage_dir}/LICENSE"

if [[ "${archive_name}" == *.zip ]]; then
  if ! command -v zip >/dev/null 2>&1; then
    echo "FAIL: zip is required for Windows packaging" >&2
    exit 1
  fi
  (
    cd "${OUT_DIR}"
    zip -rq "${archive_name}" "${PACKAGE_DIR}"
  )
else
  tar -C "${OUT_DIR}" -czf "${archive_path}" "${PACKAGE_DIR}"
fi

if command -v sha256sum >/dev/null 2>&1; then
  (
    cd "${OUT_DIR}"
    sha256sum "${archive_name}" > "${archive_name}.sha256"
  )
elif command -v shasum >/dev/null 2>&1; then
  (
    cd "${OUT_DIR}"
    shasum -a 256 "${archive_name}" > "${archive_name}.sha256"
  )
else
  echo "FAIL: sha256sum or shasum is required" >&2
  exit 1
fi

echo "archive=${archive_path}"
echo "checksum=${checksum_path}"
cat "${checksum_path}"
