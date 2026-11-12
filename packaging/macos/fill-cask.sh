#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 4 ]; then
    echo "usage: fill-cask.sh VERSION REPOSITORY_URL ASSETS_DIRECTORY OUTPUT_DIRECTORY" >&2
    exit 2
fi

version="$1"
repository="${2%/}"
assets="$3"
output="$(mkdir -p "$4" && cd "$4" && pwd)"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
download="$repository/releases/download/v$version"

arm64_image="resonate-$version-aarch64-macos.dmg"
x64_image="resonate-$version-x86_64_v3-macos.dmg"

sha256_of() {
    sha256sum -- "$assets/$1" | cut -d ' ' -f 1
}

cask="$output/resonate-$version-cask.rb"
sed \
    -e "s|@VERSION@|$version|g" \
    -e "s|@REPOSITORY_URL@|$repository|g" \
    -e "s|@DOWNLOAD_URL@|$download|g" \
    -e "s|@ARM64_IMAGE_SHA256@|$(sha256_of "$arm64_image")|g" \
    -e "s|@X64_IMAGE_SHA256@|$(sha256_of "$x64_image")|g" \
    "$here/resonate.rb" > "$cask"

if grep -Eq '@[A-Z0-9_]+@' "$cask"; then
    echo "a placeholder was left unfilled" >&2
    exit 1
fi
