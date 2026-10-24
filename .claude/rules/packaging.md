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
  `RUSTFLAGS` replaces `build.rustflags`). The release assets (spec, release PKGBUILDs, DEB script)
  build for `x86-64-v3` and nothing else: `ExclusiveArch: x86_64`, `arch=('x86_64')`, `amd64`, the
  requirement said in the Arch `pkgdesc` and the DEB description
  (`every_release_recipe_builds_for_x86_64_v3_alone`). The `-git` PKGBUILDs append `native` in
  `build` and `check` (AUR builds target the building machine) and keep `aarch64`; the Flatpak
  keeps its own baseline (`x86-64` / `generic`) and is no release asset.
- **Spec: `--locked`, built from a source archive** of the release tag, prefixed
  `resonate-<Version>/`; Fedora 44; the spec runs no tests, which CI and the maintainer's checks hold,
  since a release build that also tested compiled the workspace twice. `release.yml`'s
  `fedora-rpm` job builds the player and the server RPM and the source RPMs in one container, both
  into one `CARGO_TARGET_DIR` so the crates they share compile once; the specs and the release
  PKGBUILDs install from `${CARGO_TARGET_DIR:-target}`, and the `arch-package` job shares one the
  same way. The DEB script already shares the checkout's `target/`.
  `git archive` trusts `GITHUB_WORKSPACE` via `safe.directory` for that invocation only
  (the container refuses the runner-owned checkout). Dependencies install as root but `rpmbuild`
  runs as `rpm-builder`.
  Cargo fetches while building: a source RPM recipe, not an offline distribution build.
- **Pre-release spellings are in `release.md`.** The spec tests map `-` to `~` before comparing, the
  release PKGBUILDs hold `_version` to Cargo's and `pkgver` to `${_version//-/}`, and the DEB script
  derives its version from `Cargo.toml` itself (`the_deb_script_derives_its_version_from_cargo`).
- **`packaging/release/` holds the non-git Arch recipes** (`PKGBUILD` is `resonate-player`,
  `server/PKGBUILD` is `resonate-server`): source is the GitHub archive of tag `v$_version`
  (`sha256sums` `SKIP`, an archive's bytes being GitHub's), a local file of the same name beside
  the recipe taking its place, which is how `release.yml` builds from the tag checkout it already
  has. `resonate.install` there is a symlink to the player's. The installed layout is the `-git`
  recipe's.
- **`packaging/deb/build.sh player|server [directory]` builds the DEBs** on Ubuntu 24.04 with
  rustup stable (the distribution's cargo predates edition 2024): `cargo build --locked --release`
  under `-C target-cpu=x86-64-v3`, the spec's `%install` layout (man pages gzipped, the zsh
  completion under `vendor-completions`), `Depends` from `dpkg-shlibdeps` plus `pipewire` and
  `libvulkan1` (loaded at run time) for the player or `systemd` for the server, and `dpkg-deb
  --root-owner-group`. The server's environment file is a `conffiles` entry and its maintainer
  scripts only reload systemd and stop the unit on removal.
- **`release.yml` (`build.md`) builds these recipes in parallel jobs and `publish` attaches
  everything**, so one failing job leaves the others' assets. Asset names carry no `~` (`-` in its
  place) and the Arch packages are renamed `-x86_64_v3.pkg.tar.zst`.
- **Flatpak builds its own checkout** (`dir` source, no release tarball: metainfo names no
  `<release>`) on Freedesktop 25.08 SDK + `rust-stable`, network shared for `cargo fetch --locked`,
  no tests.
- **Sandbox = the host sockets and names the app uses**: home, `xdg-run/pipewire-0` (not
  PulseAudio), own+talk `org.mpris.MediaPlayer2.resonate` and `.resonate.*`,
  `org.freedesktop.Notifications`, system-bus `org.bluez` (headset controls, `mpris.md`),
  `discord-ipc-0`..`-9` plus Discord and Vesktop runtime directories. No `--share=ipc`, no `x11` or
  `fallback-x11`: gpui is built with `wayland` alone (`the_flatpak_grants_no_x11_to_a_wayland_only_build`).
- **The spec's `Version` is the workspace's with `-` as `~`** (`the_rpm_spec_builds_the_version_cargo_declares`);
  the `-git` PKGBUILD's `pkgver()` reads `Cargo.toml` itself.

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
`PKGBUILD` (`resonate-server-git`) and `resonate-server.spec` (`resonate-server`), beside
`release/server/PKGBUILD` and the DEB script's `server`. All build only
`--package resonate-server` (it reaches no PipeWire, GPUI or Vulkan, so the recipes name no such
dependency), set `RUSTFLAGS` as the player's recipes do, install the unit under systemd's unit
directory and the environment file as `/etc/resonate-server/resonate-server.env`, kept across an
upgrade (`backup=` / `%config(noreplace)`), and leave the certificate and key to the listener. The
spec reads the player's source archive (`resonate-<version>.tar.gz`) and is held to the workspace's
version like the player's (`the_server_spec_builds_the_version_cargo_declares`);
`every_server_package_installs_the_unit_and_its_environment` holds every recipe to the two files.
CI builds the PKGBUILD (`server-package`: `makepkg` as a build user, `source` pointed at the runner's
checkout, `check()` included); the release recipes build only in `release.yml`.

**`packaging/.SRCINFO` = `makepkg --printsrcinfo` of the player's PKGBUILD beside it**, committed
because the AUR reads it. Regenerate it when publishing to the AUR; no test holds it, since the VCS
`pkgver` moves with every commit. The server's PKGBUILD has none until it is published.
