#!/usr/bin/env bash
set -euo pipefail

package=${1:?usage: build.sh player|server [output-directory]}
output=${2:-$PWD/deb-artifacts}

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$root"

cargo_version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
test -n "$cargo_version"
version="${cargo_version//-/\~}-1"
homepage=https://github.com/vibe-technologies-llc/resonate

export RUSTFLAGS="-C target-cpu=x86-64-v3"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

stage_documents() {
    local name=$1
    install -Dm644 LICENSE "$stage/usr/share/doc/$name/copyright"
    install -Dm644 COPYRIGHT "$stage/usr/share/doc/$name/COPYRIGHT"
    install -Dm644 README.md "$stage/usr/share/doc/$name/README.md"
}

shared_library_dependencies() {
    local binary=$1
    mkdir -p "$work/shlibs/debian"
    printf 'Source: resonate\n\nPackage: resonate\nArchitecture: any\n' >"$work/shlibs/debian/control"
    (cd "$work/shlibs" && dpkg-shlibdeps -O -e"$root/$binary" 2>/dev/null) | sed -n 's/^shlibs:Depends=//p'
}

write_control() {
    local name=$1 section=$2 depends=$3 description=$4 long=$5
    local size
    size=$(du -sk "$stage" | cut -f1)
    mkdir -p "$stage/DEBIAN"
    {
        printf 'Package: %s\n' "$name"
        printf 'Version: %s\n' "$version"
        printf 'Architecture: amd64\n'
        printf 'Maintainer: Resonate\n'
        printf 'Section: %s\n' "$section"
        printf 'Priority: optional\n'
        printf 'Installed-Size: %s\n' "$size"
        printf 'Depends: %s\n' "$depends"
        printf 'Homepage: %s\n' "$homepage"
        printf 'Description: %s\n' "$description"
        printf ' %s\n' "$long"
    } >"$stage/DEBIAN/control"
}

seal() {
    local name=$1
    mkdir -p "$output"
    dpkg-deb --root-owner-group --build "$stage" "$output/${name}_${version}_amd64.deb"
}

build_player() {
    cargo build --locked --release --workspace --bin resonate

    local generated
    generated=$(find target/release/build -name resonate.1 -printf '%h\n' -quit)
    test -n "$generated"

    stage="$work/resonate"
    install -Dm755 target/release/resonate "$stage/usr/bin/resonate"
    install -Dm644 packaging/resonate.desktop "$stage/usr/share/applications/resonate.desktop"
    install -Dm644 packaging/resonate.svg "$stage/usr/share/icons/hicolor/scalable/apps/resonate.svg"
    install -Dm644 packaging/org.resonate.Resonate.metainfo.xml "$stage/usr/share/metainfo/org.resonate.Resonate.metainfo.xml"
    install -Dm644 -t "$stage/usr/share/man/man1" "$generated"/*.1
    install -Dm644 -t "$stage/usr/share/man/man5" packaging/resonate-config.toml.5
    gzip -9n "$stage"/usr/share/man/man1/*.1 "$stage"/usr/share/man/man5/*.5
    install -Dm644 "$generated/resonate.bash" "$stage/usr/share/bash-completion/completions/resonate"
    install -Dm644 "$generated/_resonate" "$stage/usr/share/zsh/vendor-completions/_resonate"
    install -Dm644 "$generated/resonate.fish" "$stage/usr/share/fish/vendor_completions.d/resonate.fish"
    stage_documents resonate

    local libraries
    libraries=$(shared_library_dependencies target/release/resonate)
    write_control resonate sound "${libraries:+$libraries, }pipewire (>= 0.3.50), libvulkan1" \
        'High-fidelity music player with native PipeWire output' \
        'Resonate plays music through PipeWire with a native Wayland interface. Built for x86-64-v3 processors.'
    seal resonate
}

build_server() {
    cargo build --locked --release --package resonate-server

    stage="$work/resonate-server"
    install -Dm755 target/release/resonate-server "$stage/usr/bin/resonate-server"
    install -Dm644 packaging/server/resonate-server.service "$stage/usr/lib/systemd/system/resonate-server.service"
    install -Dm644 packaging/server/resonate-server.env "$stage/etc/resonate-server/resonate-server.env"
    stage_documents resonate-server

    mkdir -p "$stage/DEBIAN"
    printf '/etc/resonate-server/resonate-server.env\n' >"$stage/DEBIAN/conffiles"
    printf '#!/bin/sh\nset -e\nif [ -d /run/systemd/system ]; then\n    systemctl daemon-reload || true\nfi\n' >"$stage/DEBIAN/postinst"
    printf '#!/bin/sh\nset -e\nif [ "$1" = remove ] && [ -d /run/systemd/system ]; then\n    systemctl stop resonate-server.service || true\nfi\n' >"$stage/DEBIAN/prerm"
    printf '#!/bin/sh\nset -e\nif [ -d /run/systemd/system ]; then\n    systemctl daemon-reload || true\nfi\n' >"$stage/DEBIAN/postrm"
    chmod 755 "$stage/DEBIAN/postinst" "$stage/DEBIAN/prerm" "$stage/DEBIAN/postrm"

    local libraries
    libraries=$(shared_library_dependencies target/release/resonate-server)
    write_control resonate-server net "${libraries:+$libraries, }systemd" \
        'Sync server for Resonate: keeps sealed records and introduces devices' \
        'The Resonate sync server keeps records its devices sealed under keys it never holds, and introduces devices that share a pairing code. It runs as a hardened systemd service with a dynamic user. Built for x86-64-v3 processors.'
    seal resonate-server
}

case "$package" in
    player) build_player ;;
    server) build_server ;;
    *) echo "unknown package: $package" >&2; exit 2 ;;
esac
