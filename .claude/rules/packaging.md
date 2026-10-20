---
paths:
  - "packaging/**"
---

# Packaging

PKGBUILD, RPM spec and Flatpak manifest all build the default-feature `resonate` binary plus
desktop entry, icon, metainfo, man pages, completions. The `resonate*.1` man pages and completions
come from the binary crate's build script, so a CLI change reaches every package with no second
list; `resonate-config.toml.5`, the settings file's manual, is written by hand and installed from
`packaging/` by each recipe (`every_package_installs_the_manual_of_the_settings_file`, and
`binary.md` for what holds its keys in step). Desktop
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

**`packaging/server/` deploys the sync server, not the player** (`server.md`): a hardened systemd
unit, its environment file and the two recipes that install them with the `resonate-server` binary:
`PKGBUILD` (`resonate-server-git`) and `resonate-server.spec` (`resonate-server`). Both build only
`--package resonate-server` (it reaches no PipeWire, GPUI or Vulkan, so the recipes name no such
dependency), set `RUSTFLAGS` as the player's recipes do, install the unit under systemd's unit
directory and the environment file as `/etc/resonate-server/resonate-server.env`, kept across an
upgrade (`backup=` / `%config(noreplace)`), and leave the certificate and key to the listener. The
spec reads the player's source archive (`resonate-<version>.tar.gz`) and is held to the workspace's
version like the player's (`the_server_spec_builds_the_version_cargo_declares`);
`every_server_package_installs_the_unit_and_its_environment` holds both recipes to the two files.
CI builds the PKGBUILD (`server-package`: `makepkg` as a build user, `source` pointed at the runner's
checkout, `check()` included); `rpm-release.yml`'s own job, `fedora-server-rpm`, builds the spec on a
release or manual run apart from the player's, so one failing leaves the other's RPM.

**`packaging/.SRCINFO` = `makepkg --printsrcinfo` of the player's PKGBUILD beside it**, committed
because the AUR reads it. Regenerate it when publishing to the AUR; no test holds it, since the VCS
`pkgver` moves with every commit. The server's PKGBUILD has none until it is published.
