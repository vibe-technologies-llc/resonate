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

- **Every package sets `RUSTFLAGS` over the checkout's `target-cpu=native`**, because any
  environment `RUSTFLAGS` replaces `build.rustflags` outright. The PKGBUILD appends
  `-C target-cpu=native` in `build` and `check`, so an AUR build is made for the machine building
  it. The spec appends `target-cpu=x86-64` on x86_64 and `generic` on aarch64, and the Flatpak
  manifest sets the same pair, so a release runs on older machines of the architecture.
- **The spec builds `--locked` from a source archive of the published release tag**, whose version
  must match the spec, or the branch selected for a manual workflow run, with development headers
  from Fedora 44. `%check` tests the headless workspace; `rpm-release.yml` keeps the binary and
  source RPMs as a downloadable workflow artifact once build and tests pass, and attaches them to
  the release on a release run, so a listener can install a build before a release is published.
  The archive command trusts `GITHUB_WORKSPACE` for that invocation alone, because the Fedora
  container can see a checkout owned by the runner's user and refuse it as dubious ownership.
  Dependencies are installed as root, but `rpmbuild` runs as `rpm-builder` under its own home,
  with the archive and RPMs in its `rpmbuild` tree. The tests exercise refused writes to read-only
  fixtures; root bypasses those permissions and writes both files in a walk meant to leave one
  for the next pass, so the build and `%check` run as an ordinary user.
  Cargo fetches crates while building: a source RPM recipe, not an offline distribution build.
- **The Flatpak builds the checkout it sits in** (a `dir` source, no release tarball, so the
  metainfo names no `<release>`) with the Freedesktop 25.08 SDK and `rust-stable`, shares the
  network for `cargo fetch --locked`, and runs no tests. The SDK supplies `libpipewire`, `libspa`
  and libclang (`libspa-sys` bindgen), Wayland, Vulkan, fontconfig and freetype.
- **Sandbox permissions are the host's sockets and names the app uses**: the home directory (a
  library there can be scanned, tagged and organised; elsewhere via the file portal or an
  override), `xdg-run/pipewire-0` (PipeWire, not PulseAudio), the owned and talk names
  `org.mpris.MediaPlayer2.resonate` and `org.mpris.MediaPlayer2.resonate.*` (a Flatpak name may own
  `org.mpris.MediaPlayer2.` plus its own id, and this player claims `resonate`),
  `org.freedesktop.Notifications`, the system bus's `org.bluez` (a headset's own controls, `mpris.md`),
  and `discord-ipc-0` to `-9` with the Discord and Vesktop runtime directories. A Snap Discord lives on the host `/tmp`, outside the sandbox.

**Every package installs the one `resonate.desktop` and `resonate.svg`, and the Flatpak renames
them as it builds.** flatpak-builder exports only what is named after the app id, so the manifest
carries `rename-desktop-file: resonate.desktop` and `rename-icon: resonate`; the entry becomes
`org.resonate.Resonate.desktop` and the icon `org.resonate.Resonate.svg`, and the metainfo's
`<launchable type="desktop-id">`, which names `resonate.desktop` as the other packages install it,
is rewritten to match. `StartupWMClass=resonate` is left alone so the window's `APP_ID` still
matches it, and the file in `packaging/` keeps the name the window's tests read it by.
`crates/resonate/tests/packaging.rs` holds the manifest to this. The bus's `DesktopEntry` and a
notification's `desktop-entry` hint follow the rename: `mpris.rs` answers `FLATPAK_ID` where it is
set and `resonate` elsewhere, so a shell matching player to launcher finds the installed entry.

**`packaging/.SRCINFO` is `makepkg --printsrcinfo`'s output for the PKGBUILD beside it**, committed
because the AUR reads it rather than the PKGBUILD. A change to the PKGBUILD regenerates it in the
same commit; `the_srcinfo_says_what_the_pkgbuild_says` fails where the two disagree.
