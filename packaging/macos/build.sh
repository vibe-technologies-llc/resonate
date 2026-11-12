#!/usr/bin/env bash
set -euo pipefail

version=${1:-}
output=${2:-assets}
architecture=${3:-aarch64}

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root"

if [ -z "$version" ]; then
    version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
fi
test -n "$version"
numeric=$(printf '%s' "$version" | sed -n 's/^\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\).*$/\1/p')
test -n "$numeric"

case "$architecture" in
    aarch64)
        triple=aarch64-apple-darwin
        named=aarch64
        flags=""
        ;;
    x86_64)
        triple=x86_64-apple-darwin
        named=x86_64_v3
        flags="-C target-cpu=x86-64-v3"
        ;;
    *)
        echo "usage: build.sh [VERSION] [OUTPUT] [aarch64|x86_64]" >&2
        exit 2
        ;;
esac
inter_archive=https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip
inter_sha256=9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e
jetbrains_mono_archive=https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip
jetbrains_mono_sha256=6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf
images_tried_at_most=3

export RUSTFLAGS="$flags"
export MACOSX_DEPLOYMENT_TARGET=13.0

rustup target add "$triple"
cargo build --release --locked --target "$triple" --package resonate --bin resonate
cargo build --release --locked --target "$triple" --package resonate-signin

release="target/$triple/release"
generated=$(find "$release/build" -name resonate.1 -print -quit)
test -n "$generated"
generated=$(dirname "$generated")

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fetched() {
    local address=$1 sha256=$2 archive=$3
    curl --fail --location --silent --show-error --output "$archive" "$address"
    printf '%s  %s\n' "$sha256" "$archive" | shasum -a 256 -c - >/dev/null
}

staging="$work/Resonate"
application="$staging/Resonate.app"
contents="$application/Contents"
resources="$contents/Resources"
mkdir -p "$contents/MacOS" "$resources/Fonts" "$resources/man/man1" "$resources/man/man5" "$resources/completions"

cp "$release/resonate" "$contents/MacOS/resonate"
cp "$release/resonate-signin" "$contents/MacOS/resonate-signin"
sed "s/@VERSION@/$numeric/g" "$here/Info.plist" >"$contents/Info.plist"
cp "$here/resonate.icns" "$resources/resonate.icns"
cp LICENSE COPYRIGHT "$resources/"
cp "$generated"/*.1 "$resources/man/man1/"
cp packaging/resonate-config.toml.5 "$resources/man/man5/"
cp "$generated/resonate.bash" "$generated/_resonate" "$generated/resonate.fish" "$resources/completions/"

fetched "$inter_archive" "$inter_sha256" "$work/inter.zip"
unzip -q "$work/inter.zip" 'extras/ttf/Inter-*.ttf' LICENSE.txt -d "$work/inter"
cp "$work/inter/extras/ttf"/Inter-*.ttf "$resources/Fonts/"
cp "$work/inter/LICENSE.txt" "$resources/Fonts/Inter-LICENSE.txt"

fetched "$jetbrains_mono_archive" "$jetbrains_mono_sha256" "$work/jetbrains-mono.zip"
unzip -q "$work/jetbrains-mono.zip" 'fonts/ttf/JetBrainsMono-*.ttf' OFL.txt -d "$work/jetbrains-mono"
cp "$work/jetbrains-mono/fonts/ttf"/JetBrainsMono-*.ttf "$resources/Fonts/"
cp "$work/jetbrains-mono/OFL.txt" "$resources/Fonts/JetBrainsMono-OFL.txt"

"$here/sign.sh" "$application"

ln -s /Applications "$staging/Applications"
cp "$here/README.txt" LICENSE COPYRIGHT "$staging/"

mkdir -p "$output"
image="$output/resonate-$version-$named-macos.dmg"
tried=0
until hdiutil create -volname Resonate -srcfolder "$staging" -format UDZO -ov "$image" >/dev/null; do
    tried=$((tried + 1))
    test "$tried" -lt "$images_tried_at_most"
    sleep 5
done

"$here/sign.sh" --image "$image"
printf '%s\n' "$image"
