#!/usr/bin/env bash
# Build the Vex.app bundle and create a distributable .dmg — ADR-024 Phase H PH-03.
#
# Usage:
#   bash packaging/macos/build-app.sh <vex-binary> <launcher-binary> <version>
#
#   vex-binary       Path to the compiled vex binary for the target architecture.
#   launcher-binary  Path to the compiled vex-launcher binary.
#   version          Release version string, e.g. v0.1.0-beta.6.
#
# Environment variables:
#   APPLE_DEVELOPER_ID_CERT  Developer ID Application identity string for codesign.
#                            When absent, signing is skipped and the build is
#                            labelled "unsigned development build".
#   APPLE_NOTARYTOOL_KEY     Path to the App Store Connect API key file (.p8).
#   APPLE_NOTARYTOOL_KEY_ID  App Store Connect API key ID.
#   APPLE_NOTARYTOOL_ISSUER  App Store Connect issuer ID.
#
# Outputs:
#   dist/Vex.app/                The assembled app bundle.
#   dist/vex-<version>-macos.dmg The distributable disk image.
set -euo pipefail

VEX_BINARY="${1:?usage: $0 <vex-binary> <launcher-binary> <version>}"
LAUNCHER_BINARY="${2:?usage: $0 <vex-binary> <launcher-binary> <version>}"
VERSION="${3:?usage: $0 <vex-binary> <launcher-binary> <version>}"

APP_NAME="Vex.app"
BUNDLE_DIR="dist/${APP_NAME}"
CONTENTS="${BUNDLE_DIR}/Contents"
MACOS_DIR="${CONTENTS}/MacOS"
DMG_NAME="vex-${VERSION}-macos.dmg"
SIGNED=false

echo "[build-app] version: ${VERSION}"
echo "[build-app] vex binary: ${VEX_BINARY}"
echo "[build-app] launcher binary: ${LAUNCHER_BINARY}"

# --- Assemble the .app bundle -----------------------------------------------

echo "[build-app] assembling ${BUNDLE_DIR}"
rm -rf "${BUNDLE_DIR}"
mkdir -p "${MACOS_DIR}"

cp "${VEX_BINARY}" "${MACOS_DIR}/vex"
chmod +x "${MACOS_DIR}/vex"

cp "${LAUNCHER_BINARY}" "${MACOS_DIR}/vex-launcher"
chmod +x "${MACOS_DIR}/vex-launcher"

# Substitute __VERSION__ in Info.plist and copy it into the bundle.
sed "s/__VERSION__/${VERSION}/g" packaging/macos/Info.plist > "${CONTENTS}/Info.plist"

echo "[build-app] bundle assembled"

# --- Code signing (PH-03) ----------------------------------------------------

if [[ -n "${APPLE_DEVELOPER_ID_CERT:-}" ]]; then
    echo "[build-app] signing with Developer ID: ${APPLE_DEVELOPER_ID_CERT}"
    # Sign the inner binaries first, then the bundle as a whole.
    codesign \
        --force \
        --sign "${APPLE_DEVELOPER_ID_CERT}" \
        --options runtime \
        --timestamp \
        "${MACOS_DIR}/vex"
    codesign \
        --force \
        --sign "${APPLE_DEVELOPER_ID_CERT}" \
        --options runtime \
        --timestamp \
        "${MACOS_DIR}/vex-launcher"
    codesign \
        --force \
        --deep \
        --sign "${APPLE_DEVELOPER_ID_CERT}" \
        --options runtime \
        --timestamp \
        "${BUNDLE_DIR}"
    SIGNED=true
    echo "[build-app] code signing complete"
else
    echo "[build-app] APPLE_DEVELOPER_ID_CERT not set — skipping code signing (unsigned development build)"
fi

# --- Notarisation (PH-03) ----------------------------------------------------

if [[ "${SIGNED}" == "true" ]] \
    && [[ -n "${APPLE_NOTARYTOOL_KEY:-}" ]] \
    && [[ -n "${APPLE_NOTARYTOOL_KEY_ID:-}" ]] \
    && [[ -n "${APPLE_NOTARYTOOL_ISSUER:-}" ]]; then

    echo "[build-app] creating temporary zip for notarisation"
    NOTARY_ZIP="dist/vex-notarize.zip"
    ditto -c -k --keepParent "${BUNDLE_DIR}" "${NOTARY_ZIP}"

    echo "[build-app] submitting to notary service"
    xcrun notarytool submit "${NOTARY_ZIP}" \
        --key "${APPLE_NOTARYTOOL_KEY}" \
        --key-id "${APPLE_NOTARYTOOL_KEY_ID}" \
        --issuer "${APPLE_NOTARYTOOL_ISSUER}" \
        --wait

    echo "[build-app] stapling notarisation ticket"
    xcrun stapler staple "${BUNDLE_DIR}"

    rm -f "${NOTARY_ZIP}"
    echo "[build-app] notarisation complete"
else
    if [[ "${SIGNED}" == "true" ]]; then
        echo "[build-app] notarisation credentials not set — skipping notarisation"
    fi
fi

# --- Create .dmg -------------------------------------------------------------

echo "[build-app] creating ${DMG_NAME}"
hdiutil create \
    -volname "Vex ${VERSION}" \
    -srcfolder "${BUNDLE_DIR}" \
    -ov \
    -format UDZO \
    "dist/${DMG_NAME}"

echo "[build-app] done: dist/${DMG_NAME}"
