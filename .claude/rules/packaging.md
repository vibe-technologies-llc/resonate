---
paths:
  - "packaging/**"
---

# Packaging

PKGBUILD, RPM spec and Flatpak manifest all build the default-feature `resonate` binary plus
desktop entry, icon, metainfo, man pages, completions. Man pages and completions come from the
binary crate's build script, so a CLI change reaches every package with no second list. Desktop
entry and metainfo `<provides>` are held to `MIME_TYPES` by tests (`binary.md`).

- **Every package sets `RUSTFLAGS` over the checkout's `target-cpu=native`** (environment
  `RUSTFLAGS` replaces `build.rustflags`). PKGBUILD appends `native` in `build` and `check` (AUR
  builds target the building machine); spec and Flatpak use `x86-64` (x86_64) / `generic` (aarch64),
  since releases run on older machines.
- **Spec: `--locked`, built from a source archive** of the release tag (version must match the
  spec) or, on manual run, the selected branch; Fedora 44; `%check` tests the headless workspace.
  `rpm-release.yml` keeps both RPMs as a workflow artifact and attaches them to the release on a
  release run. `git archive` trusts `GITHUB_WORKSPACE` via `safe.directory` for that invocation only
  (the container refuses the runner-owned checkout). Dependencies install as root but `rpmbuild`
  runs as `rpm-builder`: tests exercise refused writes to read-only fixtures and root bypasses them.
  Cargo fetches while building: a source RPM recipe, not an offline distribution build.
- **Flatpak builds its own checkout** (`dir` source, no release tarball: metainfo names no
  `<release>`) on Freedesktop 25.08 SDK + `rust-stable`, network shared for `cargo fetch --locked`,
  no tests.
- **Sandbox = the host sockets and names the app uses**: home, `xdg-run/pipewire-0` (not
  PulseAudio), own+talk `org.mpris.MediaPlayer2.resonate` and `.resonate.*`,
  `org.freedesktop.Notifications`, system-bus `org.bluez` (headset controls, `mpris.md`),
  `discord-ipc-0`..`-9` plus Discord and Vesktop runtime directories. No `--share=ipc`, no `x11` or
  `fallback-x11`: gpui is built with `wayland` alone (`the_flatpak_grants_no_x11_to_a_wayland_only_build`).
- **The spec's `Version` is the workspace's** (`the_rpm_spec_builds_the_version_cargo_declares`);
  the PKGBUILD's `pkgver()` reads `Cargo.toml` itself.

**Every package installs one `resonate.desktop` and `resonate.svg`; the Flatpak renames them
while building.** flatpak-builder exports only app-id-named files, so the manifest has
`rename-desktop-file: resonate.desktop`, `rename-icon: resonate`, giving
`org.resonate.Resonate.desktop` and `.svg`; metainfo `<launchable type="desktop-id">` is rewritten
to match. `StartupWMClass=resonate` stays (window `APP_ID` matches). Held by
`crates/resonate/tests/packaging.rs` (`the_flatpak_exports_the_desktop_entry_and_icon_under_its_id`).
Bus `DesktopEntry` and the notification `desktop-entry` hint follow: `mpris.rs` answers
`FLATPAK_ID` where set, else `resonate`.

**`packaging/.SRCINFO` = `makepkg --printsrcinfo` of the PKGBUILD beside it**, committed because
the AUR reads it. Regenerate in the same commit as any PKGBUILD change;
`the_srcinfo_says_what_the_pkgbuild_says` fails on disagreement.
