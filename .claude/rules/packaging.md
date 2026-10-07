---
paths:
  - "packaging/**"
---

# Packaging

PKGBUILD, RPM spec and Flatpak manifest all build the default-feature `resonate` binary plus
desktop entry, icon, metainfo, man pages, completions. Man pages/completions come from the binary
crate's build script (`target/release/build/resonate-*/out`): a CLI change reaches every package
with no second list. Desktop entry and metainfo `<provides>` are held to `MIME_TYPES` by tests
(`binary.md`).

- **Every package sets `RUSTFLAGS` over the checkout's `target-cpu=native`** (environment
  `RUSTFLAGS` replaces `build.rustflags`). PKGBUILD appends `-C target-cpu=native` in `build` and
  `check` (AUR builds target the building machine); spec appends `target-cpu=x86-64` (x86_64) /
  `generic` (aarch64), Flatpak manifest sets the same pair (releases run on older machines).
- **Spec: `--locked`, from a source archive** of the published release tag (version must match the
  spec) or, on manual run, the selected branch; Fedora 44 headers; `%check` tests the headless
  workspace. `rpm-release.yml` keeps both RPMs as a workflow artifact once build and tests pass, and
  attaches them to the release on a release run (installable pre-publication). `git archive` trusts
  `GITHUB_WORKSPACE` via `safe.directory` for that invocation only (Fedora container refuses the
  runner-owned checkout as dubious ownership). Dependencies install as root; `rpmbuild` runs as
  `rpm-builder` in its own home (archive and RPMs in its `rpmbuild` tree): tests exercise refused
  writes to read-only fixtures and root bypasses them, writing both files in a walk meant to leave
  one for the next pass, so build and `%check` run as an ordinary user. Cargo
  fetches while building: a source RPM recipe, not an offline distribution build.
- **Flatpak builds its own checkout** (`dir` source, no release tarball: metainfo names no
  `<release>`) on Freedesktop 25.08 SDK + `rust-stable`, network shared for `cargo fetch --locked`,
  no tests. SDK supplies `libpipewire`, `libspa`, libclang (`libspa-sys` bindgen), Wayland, Vulkan,
  fontconfig, freetype.
- **Sandbox = the host sockets and names the app uses**: home (library there scans/tags/organises;
  elsewhere via file portal or override), `xdg-run/pipewire-0` (not PulseAudio), own+talk
  `org.mpris.MediaPlayer2.resonate` and `.resonate.*` (a Flatpak name may own
  `org.mpris.MediaPlayer2.` plus its id; this player claims `resonate`),
  `org.freedesktop.Notifications`, system-bus `org.bluez` (headset controls, `mpris.md`),
  `discord-ipc-0`..`-9` plus Discord and Vesktop runtime directories. Snap Discord lives on host
  `/tmp`, outside the sandbox.

**Every package installs one `resonate.desktop` and `resonate.svg`; the Flatpak renames them
while building.** flatpak-builder exports only app-id-named files, so the manifest has
`rename-desktop-file: resonate.desktop`, `rename-icon: resonate`: entry →
`org.resonate.Resonate.desktop`, icon → `org.resonate.Resonate.svg`; metainfo
`<launchable type="desktop-id">` (`resonate.desktop` for other packages) is rewritten to match.
`StartupWMClass=resonate` stays (window `APP_ID` matches); the `packaging/` file keeps the name the
window's tests read. `crates/resonate/tests/packaging.rs` holds this
(`the_flatpak_exports_the_desktop_entry_and_icon_under_its_id`). Bus `DesktopEntry` and
notification `desktop-entry` hint follow: `mpris.rs` answers `FLATPAK_ID` where set, else
`resonate` (a shell matching player to launcher finds the installed entry).

**`packaging/.SRCINFO` = `makepkg --printsrcinfo` of the PKGBUILD beside it**, committed because
the AUR reads it, not the PKGBUILD. Regenerate in the same commit as any PKGBUILD change;
`the_srcinfo_says_what_the_pkgbuild_says` fails on disagreement.
