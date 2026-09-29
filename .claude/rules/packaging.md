---
paths:
  - "packaging/**"
---

# Packaging

The Arch PKGBUILD, the Fedora RPM spec and the Flatpak manifest build the default-feature
`resonate` binary and install the desktop entry, icon, metainfo, man pages and shell completions
together. The man pages and completions come from the binary crate's build script under
`target/release/build/resonate-*/out`, so a CLI change reaches every package with no second list.
The desktop entry and the metainfo's `<provides>` are held to the binary's `MIME_TYPES` by tests
(`binary.md`).

Every package build exports `RUSTFLAGS` during build and test, or sets it in the manifest. The
checkout's `.cargo/config.toml` uses `target-cpu=native` for local performance, and the Arch
PKGBUILD appends `-C target-cpu=native` in `build` and `check` — appending, because any
environment `RUSTFLAGS` replaces `build.rustflags` outright and makepkg always sets one — so an AUR
build is made for, and only for, the machine building it. The Fedora spec appends
`target-cpu=x86-64` on x86_64 and `target-cpu=generic` on aarch64, after any Fedora flags, so a
release runs on older machines of the architecture; the Flatpak manifest sets the same pair, its
value being what the sandbox build sees. The spec builds `--locked` from a source archive of the
published release tag, whose version must match the spec, with development headers from Fedora
44; `%check` tests the headless workspace, and the release workflow uploads the binary and source
RPMs once build and tests pass. Cargo fetches crates while building: a source RPM recipe, not an
offline Fedora distribution build.

`packaging/org.resonate.Resonate.yml` builds the checkout it sits in, with the Freedesktop 25.08
SDK and the `rust-stable` extension. The SDK supplies `libpipewire`, `libspa` and libclang (which
`libspa-sys` bindgen needs), and Wayland, Vulkan, fontconfig and freetype for the window. The build
shares the network so `cargo fetch --locked` reaches crates.io, and runs no tests. The sandbox sees
the home directory, so a library there can be scanned, tagged and organised; a folder outside is
reached through the file portal or a Flatpak override. PipeWire is the host socket
`xdg-run/pipewire-0`, not PulseAudio. The bus names are `org.mpris.MediaPlayer2.resonate` and
`org.mpris.MediaPlayer2.resonate.*`, a Flatpak name being allowed to own `org.mpris.MediaPlayer2.`
plus its own id and this player claiming `resonate`. Notifications talk to
`org.freedesktop.Notifications`. Discord presence sees the host `discord-ipc-0` to `discord-ipc-9`
sockets and the Discord and Vesktop runtime directories the client searches; a Snap install of
Discord lives on the host `/tmp`, outside the sandbox. No release is published, so the manifest
takes the directory rather than a tarball and the metainfo names no `<release>`.

**Every package installs the one `resonate.desktop` and `resonate.svg`, and the Flatpak renames
them as it builds.** flatpak-builder exports only what is named after the app id, so the manifest
carries `rename-desktop-file: resonate.desktop` and `rename-icon: resonate`: the entry becomes
`org.resonate.Resonate.desktop` with `X-Flatpak-RenamedFrom=resonate.desktop;` and
`Icon=org.resonate.Resonate`, the icon `org.resonate.Resonate.svg`, and the metainfo's
`<launchable type="desktop-id">`, which names `resonate.desktop` as the other packages install it,
is rewritten to the renamed one. `StartupWMClass=resonate` is left alone, so the window's `APP_ID`
still matches it under Flatpak, and the file in `packaging/` keeps the name the window's tests read
it by. `crates/resonate/tests/packaging.rs` holds the manifest to this
(`the_flatpak_exports_the_desktop_entry_and_icon_under_its_id`).

**`packaging/.SRCINFO` is `makepkg --printsrcinfo`'s output for the PKGBUILD beside it**, committed
because the AUR reads it rather than the PKGBUILD; a change to the PKGBUILD regenerates it in the
same commit, and `the_srcinfo_says_what_the_pkgbuild_says` fails where `pkgver`, `pkgrel`, `arch`,
`license`, `depends`, `makedepends` or `options` disagree.
