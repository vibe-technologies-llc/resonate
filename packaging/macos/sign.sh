#!/usr/bin/env bash
set -euo pipefail

usage='usage: sign.sh APPLICATION | sign.sh --image DISK_IMAGE'
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

certificate=${RESONATE_MACOS_CERTIFICATE_BASE64:-}
certificate_password=${RESONATE_MACOS_CERTIFICATE_PASSWORD:-}
notary_key=${RESONATE_MACOS_NOTARY_KEY_BASE64:-}
notary_key_id=${RESONATE_MACOS_NOTARY_KEY_ID:-}
notary_issuer=${RESONATE_MACOS_NOTARY_ISSUER:-}

work=$(mktemp -d)
keychain="$work/signing.keychain-db"
keychains_before=
identity=

restore() {
    if [ -n "$keychains_before" ]; then
        printf '%s\n' "$keychains_before" | xargs security list-keychains -d user -s
    fi
    if [ -e "$keychain" ]; then
        security delete-keychain "$keychain"
    fi
    rm -rf "$work"
}
trap restore EXIT

holds_a_developer_id() {
    [ -n "$certificate" ] && [ -n "$certificate_password" ]
}

holds_a_notary_key() {
    [ -n "$notary_key" ] && [ -n "$notary_key_id" ] && [ -n "$notary_issuer" ]
}

take_the_developer_id() {
    local keychain_password
    keychain_password=$(uuidgen)
    printf '%s' "$certificate" | base64 --decode >"$work/certificate.p12"
    security create-keychain -p "$keychain_password" "$keychain"
    security set-keychain-settings -lut 3600 "$keychain"
    security unlock-keychain -p "$keychain_password" "$keychain"
    security import "$work/certificate.p12" -k "$keychain" -P "$certificate_password" -T /usr/bin/codesign
    security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$keychain" >/dev/null
    keychains_before=$(security list-keychains -d user | sed 's/^ *"\(.*\)"$/\1/')
    printf '%s\n%s\n' "$keychain" "$keychains_before" | xargs security list-keychains -d user -s
    identity=$(security find-identity -v -p codesigning "$keychain" | sed -n 's/^ *[0-9]*) \([0-9A-F]\{40\}\) .*$/\1/p' | head -1)
    test -n "$identity"
}

sign_the_application() {
    local application=$1
    local helper="$application/Contents/MacOS/resonate-signin"
    test -d "$application"
    if holds_a_developer_id; then
        take_the_developer_id
        codesign --force --options runtime --timestamp --sign "$identity" "$helper"
        codesign --force --options runtime --timestamp --entitlements "$here/entitlements.plist" --sign "$identity" "$application"
    else
        echo "no Developer ID certificate is configured, so $application is signed ad hoc and Gatekeeper asks before a first run"
        codesign --force --sign - "$helper"
        codesign --force --sign - "$application"
    fi
    codesign --verify --strict "$application"
}

sign_the_image() {
    local image=$1
    test -f "$image"
    if ! holds_a_developer_id; then
        echo "no Developer ID certificate is configured, so $image is neither signed nor notarised"
        return
    fi
    take_the_developer_id
    codesign --force --timestamp --sign "$identity" "$image"
    if ! holds_a_notary_key; then
        echo "no notary key is configured, so $image is signed and not notarised"
        return
    fi
    printf '%s' "$notary_key" | base64 --decode >"$work/notary.p8"
    xcrun notarytool submit "$image" --key "$work/notary.p8" --key-id "$notary_key_id" --issuer "$notary_issuer" --wait
    xcrun stapler staple "$image"
}

case "${1:-}" in
    --image) sign_the_image "${2:?$usage}" ;;
    '') echo "$usage" >&2; exit 2 ;;
    *) sign_the_application "$1" ;;
esac
