#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 4 ]; then
    echo "usage: fill-manifests.sh VERSION REPOSITORY_URL ASSETS_DIRECTORY OUTPUT_DIRECTORY" >&2
    exit 2
fi

version="$1"
repository="${2%/}"
assets="$3"
output="$(mkdir -p "$4" && cd "$4" && pwd)"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
download="$repository/releases/download/v$version"

x64_archive="resonate-$version-x86_64_v3-windows.zip"
arm64_archive="resonate-$version-aarch64-windows.zip"
x64_installer="resonate-$version-x86_64_v3-windows-setup.exe"
arm64_installer="resonate-$version-aarch64-windows-setup.exe"

sha256_of() {
    sha256sum -- "$assets/$1" | cut -d ' ' -f 1
}

app_id="$(sed -n 's/^AppId={{\(.*\)}$/\1/p' "$here/resonate.iss")"
if [ -z "$app_id" ]; then
    echo "resonate.iss declares no AppId" >&2
    exit 1
fi

fill() {
    sed \
        -e "s|@VERSION@|$version|g" \
        -e "s|@REPOSITORY_URL@|$repository|g" \
        -e "s|@PRODUCT_CODE@|{$app_id}_is1|g" \
        -e "s|@X64_ARCHIVE_URL@|$download/$x64_archive|g" \
        -e "s|@X64_ARCHIVE_SHA256@|$(sha256_of "$x64_archive")|g" \
        -e "s|@ARM64_ARCHIVE_URL@|$download/$arm64_archive|g" \
        -e "s|@ARM64_ARCHIVE_SHA256@|$(sha256_of "$arm64_archive")|g" \
        -e "s|@X64_INSTALLER_URL@|$download/$x64_installer|g" \
        -e "s|@X64_INSTALLER_SHA256@|$(sha256_of "$x64_installer")|g" \
        -e "s|@ARM64_INSTALLER_URL@|$download/$arm64_installer|g" \
        -e "s|@ARM64_INSTALLER_SHA256@|$(sha256_of "$arm64_installer")|g" \
        "$1"
}

winget="$(mktemp -d)"
trap 'rm -rf "$winget"' EXIT
manifests="$winget/manifests/r/Resonate/Resonate/$version"
mkdir -p "$manifests" "$output"

for template in "$here"/winget/*.yaml; do
    fill "$template" > "$manifests/$(basename "$template")"
done

(cd "$winget" && zip -q -r "$output/resonate-$version-winget-manifests.zip" manifests)

fill "$here/scoop.json" > "$output/resonate-$version-scoop.json"

if grep -rEq '@[A-Z0-9_]+@' "$manifests" "$output/resonate-$version-scoop.json"; then
    echo "a placeholder was left unfilled" >&2
    exit 1
fi
