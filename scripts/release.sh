#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  TARGET=x86_64-unknown-linux-gnu make release
  TARGET=x86_64-unknown-linux-gnu bash scripts/release.sh
  VERSION=v0.1.0-alpha2 TARGET=x86_64-unknown-linux-gnu bash scripts/release.sh

Inputs:
  VERSION / arg1   optional semver version or tag; defaults to the Cargo package tag
  TARGET  / arg2   Rust target triple to package
  OUT_DIR / arg3   output directory (default: dist)
  BUILD_TOOL       cargo or cross (default: cargo)
USAGE
}

normalize_version() {
  local version="$1"
  if [[ "${version}" != v* ]]; then
    version="v${version}"
  fi
  printf '%s\n' "${version}"
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

if [[ ! "${VERSION}" =~ ^v?[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
  echo "FAIL: VERSION must look like v0.1.0 or v0.1.0-alpha2" >&2
  exit 1
fi

if [[ "${VERSION}" != "${EXPECTED_VERSION}" ]]; then
  echo "FAIL: VERSION ${VERSION} does not match Cargo package tag ${EXPECTED_VERSION}" >&2
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

"${BUILD_TOOL}" build --release --target "${TARGET}"

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
