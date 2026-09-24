---
paths:
  - "packaging/**"
---

# Packaging

The Arch PKGBUILD, the Fedora RPM spec and the Flatpak manifest build the default-feature
`resonate` binary and install the desktop entry, icon, metainfo, man pages and shell completions
together. The man pages and completions come from the binary crate's build script under
`target/release/build/resonate-*/out`, so a CLI change reaches every package without a second
list to keep in step.

Every package build exports `RUSTFLAGS` during build and test, or sets it in the manifest. The
checkout's `.cargo/config.toml` uses `target-cpu=native` for local performance, and the Arch
PKGBUILD keeps that for an AUR build made on the listener's machine. The Fedora spec appends
`target-cpu=x86-64` for x86_64 and `target-cpu=generic` for aarch64, after any Fedora flags, so
a release runs on older machines of the same architecture. The Flatpak manifest sets the same
pair, because an environment `RUSTFLAGS` replaces `build.rustflags` outright and the manifest's
value is what the sandbox build sees. The spec builds from a source archive of the published
release tag, whose version must match the spec, takes development headers from Fedora 44, and
uses Cargo's locked dependency graph. The release workflow uploads the binary and source RPMs after
the build and tests pass. Cargo fetches crates while building; this is a source RPM recipe, not
an offline Fedora distribution build.

`packaging/org.resonate.Resonate.yml` builds the checkout it sits in, with the Freedesktop 25.08
SDK and the `rust-stable` extension. The SDK supplies `libpipewire`, `libspa` and libclang, which
`libspa-sys` bindgen needs, and Wayland, Vulkan, fontconfig and freetype for the window. The
build shares the network so `cargo fetch --locked` can reach crates.io, and it does not run the
test suite. The sandbox sees the home directory, so a library kept there can be scanned, tagged
and organised, and a folder outside it is reached through the file portal or a Flatpak override.
PipeWire is the host socket `xdg-run/pipewire-0`, not PulseAudio. The bus names are
`org.mpris.MediaPlayer2.resonate` and `org.mpris.MediaPlayer2.resonate.*`, because a Flatpak name
is allowed to own `org.mpris.MediaPlayer2.` plus its own id and this player claims `resonate`.
Notifications talk to `org.freedesktop.Notifications`. Discord presence sees the host
`discord-ipc-0` through `discord-ipc-9` sockets and the Discord and Vesktop runtime directories
the client already searches; a Snap install of Discord lives on the host `/tmp` and is outside
the sandbox. No release is published, so the manifest takes the directory rather than a tarball,
and the metainfo names no `<release>`.
